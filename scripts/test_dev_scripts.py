"""Regression checks for command failures, portability and evidence preservation."""

import base64
import importlib.util
import io
import json
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import Mock, patch

import vft_dev
import web_licenses


def load_script(name):
    spec = importlib.util.spec_from_file_location(name, vft_dev.ROOT / "scripts" / f"{name}.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class DevelopmentScripts(unittest.TestCase):
    def setUp(self):
        output = vft_dev.ROOT / "output"
        output.mkdir(exist_ok=True)
        self.temporary = tempfile.TemporaryDirectory(prefix="script-tests-", dir=output)
        self.addCleanup(self.temporary.cleanup)
        self.directory = Path(self.temporary.name)

    def test_run_preserves_arguments_and_roots_working_directory(self):
        with patch("vft_dev.subprocess.run") as command:
            vft_dev.run("tool", Path("a path with spaces"), 'literal " quote', "$value")
        command.assert_called_once_with(
            ["tool", "a path with spaces", 'literal " quote', "$value"],
            cwd=vft_dev.ROOT, check=True,
        )

    def test_workspace_check_stops_after_failure(self):
        check = load_script("check")
        with patch.object(sys, "argv", ["check.py"]), patch.object(check, "run") as command:
            command.side_effect = [None, subprocess.CalledProcessError(7, ["cargo", "fmt"])]
            with self.assertRaises(subprocess.CalledProcessError):
                check.check()
        self.assertEqual(command.call_count, 2)

    def test_main_preserves_child_exit_code(self):
        def fail():
            raise subprocess.CalledProcessError(7, "tool")
        with self.assertRaises(SystemExit) as result:
            vft_dev.main(fail)
        self.assertEqual(result.exception.code, 7)

    def test_windows_checks_fail_before_build_or_output_on_linux(self):
        commands = [
            ("local-e2e", []), ("check-tui-cli", []),
            ("remote-benchmark", ["--x", "0", "--y", "0", "--width", "10", "--height", "10",
                                  "--profile", "balanced", "--sender-fps", "15", "--cell-size", "8"]),
        ]
        for name, args in commands:
            module = load_script(name)
            with self.subTest(script=name), patch.object(sys, "platform", "linux"), \
                    patch.object(sys, "argv", [name, *args]), patch("vft_dev.subprocess.run") as command, \
                    patch.object(module, "evidence_directory") as evidence:
                with self.assertRaisesRegex(RuntimeError, "Windows desktop"):
                    module.check()
                command.assert_not_called()
                evidence.assert_not_called()

    def test_target_directory_uses_cargo_metadata(self):
        result = subprocess.CompletedProcess([], 0, '{"target_directory": "custom target"}')
        with patch("vft_dev.subprocess.run", return_value=result):
            self.assertEqual(vft_dev.target_directory(), Path("custom target"))

    def test_sender_readiness_requires_a_complete_rectangle_not_just_a_reserved_file(self):
        module = load_script("local-e2e")
        path = self.directory / "region.txt"
        self.assertIsNone(module.read_rectangle(path))
        for content in ("", "0 0", "0 0 384 216", "0 0 0 216\n", "not a rectangle\n"):
            path.write_text(content, encoding="ascii")
            self.assertIsNone(module.read_rectangle(path))
        path.write_text("", encoding="ascii")
        sender = Mock()
        sender.poll.return_value = None
        with patch.object(module.time, "sleep", side_effect=lambda _: path.write_text("-40 20 384 216\n")) as sleep:
            self.assertEqual(module.wait_for_rectangle(path, sender), (-40, 20, 384, 216))
        sleep.assert_called_once_with(0.1)
        sender.poll.return_value = 1
        with self.assertRaisesRegex(RuntimeError, "Sender exited"):
            module.wait_for_rectangle(path, sender)

    def test_wayland_harness_refuses_missing_desktop_before_build_or_evidence(self):
        module = load_script("wayland-e2e")
        for platform, environment in [("win32", {"WAYLAND_DISPLAY": "wayland-0"}), ("linux", {})]:
            with self.subTest(platform=platform), patch.object(sys, "platform", platform), \
                    patch.dict(module.os.environ, environment, clear=True), \
                    patch.object(sys, "argv", ["wayland-e2e.py"]), \
                    patch.object(module, "run") as command, \
                    patch.object(module, "evidence_directory") as evidence:
                with self.assertRaisesRegex(RuntimeError, "interactive Linux Wayland"):
                    module.check()
                command.assert_not_called()
                evidence.assert_not_called()

    def test_metadata_failure_keeps_cargo_diagnostics(self):
        error = subprocess.CalledProcessError(101, "cargo", stderr="invalid Cargo configuration\n")
        diagnostics = io.StringIO()
        with patch("vft_dev.subprocess.run", side_effect=error), patch.object(sys, "stderr", diagnostics):
            with self.assertRaises(subprocess.CalledProcessError):
                vft_dev.target_directory()
        self.assertIn("invalid Cargo configuration", diagnostics.getvalue())

    def test_executable_suffix_matches_host(self):
        for platform, suffix in [("linux", ""), ("win32", ".exe")]:
            with patch.object(sys, "platform", platform):
                self.assertEqual(vft_dev.executable(Path("target"), "verifier", "debug"),
                                 Path("target/debug") / ("verifier" + suffix))

    def test_web_build_uses_custom_target_and_preserves_unrelated_assets(self):
        build = load_script("build-web")
        root = self.directory
        assets = root / "apps/vft-send-web/web"
        assets.mkdir(parents=True)
        names = ("index.html", "style.css", "app.js", "worker.js", "bridge.js")
        for name in names:
            (assets / name).write_text((vft_dev.ROOT / "apps/vft-send-web/web" / name).read_text(encoding="utf-8"),
                                       encoding="utf-8")
        target = root / "custom target"
        for configuration in ("debug", "release"):
            wasm = target / "wasm32-unknown-unknown" / configuration / "vft_send_web.wasm"
            wasm.parent.mkdir(parents=True)
            wasm.write_bytes(configuration.encode())
        distribution = root / "output/web-sender"
        distribution.mkdir(parents=True)
        (distribution / "keep.txt").write_text("unrelated")
        # A rebuild upgrades the old multi-file distribution without touching other files.
        for name in (*names[1:], "vft_send_web.wasm"):
            (distribution / name).write_text("old asset")
        def embedded_wasm():
            html = (distribution / "index.html").read_text(encoding="utf-8")
            return base64.b64decode(re.search(r'id="vft-wasm"[^>]*>(.*?)</script>', html).group(1), validate=True)
        with patch.object(build, "ROOT", root), patch.object(build, "target_directory", return_value=target), \
                patch.object(build, "run") as command, \
                patch.object(build, "collect_notices", return_value="VFT license fixture") as notices:
            build.build()
            self.assertIn("--release", command.call_args.args)
            self.assertEqual(embedded_wasm(), b"release")
            build.build(debug=True)
            self.assertNotIn("--release", command.call_args.args)
            artifact = (distribution / "index.html").read_text(encoding="utf-8")
            self.assertIn("VFT license fixture", artifact)
            notices.side_effect = ValueError("Missing license notice")
            with self.assertRaisesRegex(ValueError, "Missing license notice"):
                build.build()
            self.assertEqual((distribution / "index.html").read_text(encoding="utf-8"), artifact)
        self.assertEqual(embedded_wasm(), b"debug")
        self.assertEqual((distribution / "keep.txt").read_text(), "unrelated")
        self.assertEqual(set(item.name for item in distribution.iterdir()), {"index.html", "keep.txt"})

    def test_web_bundle_escapes_worker_html_and_rejects_changed_boundaries(self):
        build = load_script("build-web")
        assets = self.directory
        for name in ("index.html", "style.css", "app.js", "worker.js", "bridge.js"):
            (assets / name).write_text((vft_dev.ROOT / "apps/vft-send-web/web" / name).read_text(encoding="utf-8"),
                                       encoding="utf-8")
        delimiter = '// </ScRiPt><script>"quoted"</script>\n'
        with (assets / "worker.js").open("a", encoding="utf-8") as stream:
            stream.write(delimiter)
        notices = '</pre><script>untrusted & text</script><!-- VFT_APP -->'
        html = build.bundle(assets, b"\0asm\xff", notices)
        self.assertIn('&lt;/pre&gt;&lt;script&gt;untrusted &amp; text&lt;/script&gt;', html)
        self.assertNotIn(notices, html)
        worker = re.search(r'id="vft-worker"[^>]*>(.*?)</script>', html).group(1)
        self.assertNotIn("<", worker)
        self.assertTrue(json.loads(worker).endswith(delimiter))
        original = (assets / "index.html").read_text(encoding="utf-8")
        for marker in ("<!-- VFT_APP -->", "<!-- VFT_LICENSES -->"):
            for replacement in ("", marker * 2):
                (assets / "index.html").write_text(original.replace(marker, replacement), encoding="utf-8")
                with self.assertRaisesRegex(ValueError, "exactly one web bundle marker"):
                    build.bundle(assets, b"wasm", notices)
        (assets / "index.html").write_text(original, encoding="utf-8")
        (assets / "bridge.js").write_text("export async function renamed() {}", encoding="utf-8")
        with self.assertRaisesRegex(ValueError, "exactly one web bundle marker"):
            build.bundle(assets, b"wasm", notices)

    def test_web_licenses_include_transitive_and_build_notices_but_not_dev_dependencies(self):
        license_text = (vft_dev.ROOT / "LICENSE").read_text(encoding="utf-8")
        packages = []
        for name, license_id in (("vft-send-web", "MIT"), ("vft-core", "MIT"),
                                 ("dual", "MIT OR Apache-2.0"), ("transitive", "MIT"),
                                 ("build-helper", "MIT/Apache-2.0"), ("dev-only", "unsupported")):
            directory = self.directory / name
            directory.mkdir()
            packages.append({"id": name, "name": name, "version": "1.2.3", "license": license_id,
                             "manifest_path": str(directory / "Cargo.toml")})
            if name not in ("vft-send-web", "vft-core", "dev-only"):
                filename = "LICENSE" if license_id == "MIT" else "LICENSE-MIT"
                (directory / filename).write_text(license_text, encoding="utf-8")
        extra_notice = self.directory / "transitive/NOTICE"
        extra_notice.write_text("Additional attribution: <author> & contributors", encoding="utf-8")
        def dependency(name, kind=None):
            return {"pkg": name, "dep_kinds": [{"kind": kind}]}
        metadata = {"packages": packages, "workspace_members": ["vft-send-web", "vft-core"],
                    "resolve": {"nodes": [
                        {"id": "vft-send-web", "deps": [dependency("vft-core"), dependency("dual"),
                                                         dependency("dev-only", "dev")]},
                        {"id": "vft-core", "deps": [dependency("dual")]},
                        {"id": "dual", "deps": [dependency("transitive"), dependency("build-helper", "build")]},
                        {"id": "transitive", "deps": []}, {"id": "build-helper", "deps": []},
                        {"id": "dev-only", "deps": []},
                    ]}}
        notices = web_licenses.dependency_notices(metadata)
        self.assertEqual(len(notices), 3)
        for name in ("dual", "transitive", "build-helper"):
            self.assertTrue(any(text.startswith(f"{name} 1.2.3 — MIT\n") for text in notices))
        self.assertTrue(all(license_text.strip() in text for text in notices))
        self.assertIn(extra_notice.read_text(encoding="utf-8"), "\n".join(notices))
        packages[2]["license"] = "unsupported"
        with self.assertRaisesRegex(ValueError, "unsupported web dependency license"):
            web_licenses.dependency_notices(metadata)
        packages[2]["license"] = "MIT OR Apache-2.0"
        (self.directory / "dual/LICENSE-MIT").unlink()
        with self.assertRaisesRegex(ValueError, "Missing MIT license text"):
            web_licenses.dependency_notices(metadata)

    def test_web_licenses_preserve_modern_and_legacy_rust_toolchain_notices(self):
        for modern in (True, False):
            with self.subTest(modern=modern):
                root = self.directory / str(modern)
                docs = root / "share/doc/rust"
                docs.mkdir(parents=True)
                with self.assertRaisesRegex(ValueError, "copyright notices are missing"):
                    web_licenses.toolchain_notices(root, "rustc test")
                if modern:
                    (docs / "COPYRIGHT-library.html").write_text(
                        '<html><head><title>Heading</title></head><body><h1>Rust &amp; contributors</h1>'
                        '<p>Copyright &lt;authors&gt;</p><pre>Permission text\n  retained.</pre></body></html>',
                        encoding="utf-8")
                    license_path = docs / "licenses/MIT.txt"
                    license_path.parent.mkdir()
                else:
                    (docs / "COPYRIGHT").write_text("Rust & contributors\nCopyright <authors>\n"
                                                  "Permission text\n  retained.", encoding="utf-8")
                    license_path = docs / "LICENSE-MIT"
                with self.assertRaisesRegex(ValueError, "license texts are missing"):
                    web_licenses.toolchain_notices(root, "rustc test")
                license_path.write_text("Toolchain license text", encoding="utf-8")
                notices = "\n".join(web_licenses.toolchain_notices(root, "rustc test"))
                for text in ("rustc test", "Rust & contributors", "Copyright <authors>",
                             "Permission text\n  retained.", "Toolchain license text"):
                    self.assertIn(text, notices)
                self.assertNotIn("<body>", notices)

    def test_web_check_stops_after_failed_wasm_build(self):
        check = load_script("check-web")
        with patch.object(sys, "argv", ["check-web.py"]), patch.object(check, "run") as command:
            command.side_effect = subprocess.CalledProcessError(7, "build-web")
            with self.assertRaises(subprocess.CalledProcessError):
                check.check()
        command.assert_called_once_with(sys.executable, vft_dev.ROOT / "scripts/build-web.py")

    def test_fixture_is_repeatable_across_chunk_boundary_and_create_new(self):
        first, second = self.directory / "first.bin", self.directory / "second.bin"
        vft_dev.write_fixture(first, 65537)
        vft_dev.write_fixture(second, 65537)
        self.assertEqual(first.stat().st_size, 65537)
        self.assertEqual(vft_dev.sha256(first), vft_dev.sha256(second))
        with self.assertRaises(FileExistsError):
            vft_dev.write_fixture(first, 0)
        self.assertEqual(first.stat().st_size, 65537)

    def test_csv_round_trip_and_no_overwrite(self):
        path = self.directory / "stats.csv"
        row = {"sha256": "ABC", "capture_region": "10 20 30 40", "unknown": ""}
        vft_dev.write_csv(path, [row])
        self.assertEqual(vft_dev.read_stats(path), row)
        with self.assertRaises(FileExistsError):
            vft_dev.write_csv(path, [row])

    def test_owned_child_is_stopped_and_logs_preserved_after_exception(self):
        with self.assertRaisesRegex(RuntimeError, "injected failure"):
            with vft_dev.LoggedProcess([sys.executable, "-c", "import time; time.sleep(60)"],
                                       self.directory, "owned") as process:
                raise RuntimeError("injected failure")
        self.assertIsNotNone(process.poll())
        self.assertTrue((self.directory / "owned.log").exists())
        self.assertTrue((self.directory / "owned.err").exists())

    def test_real_child_arguments_with_spaces_and_nonzero_status(self):
        argument = str(self.directory / "input with spaces.bin")
        with vft_dev.LoggedProcess([sys.executable, "-c", "import sys; print(sys.argv[1]); sys.exit(7)",
                                   argument], self.directory, "arguments") as process:
            self.assertEqual(process.wait(timeout=15), 7)
        self.assertEqual((self.directory / "arguments.log").read_text().strip(), argument)

    def test_logged_process_can_use_an_isolated_receiver_working_directory(self):
        with vft_dev.LoggedProcess([sys.executable, "-c", "import os; print(os.getcwd())"],
                                   self.directory, "receiver-cwd", cwd=self.directory) as process:
            self.assertEqual(process.wait(timeout=15), 0)
        actual = Path((self.directory / "receiver-cwd.log").read_text().strip())
        self.assertEqual(actual.resolve(), self.directory.resolve())


if __name__ == "__main__":
    unittest.main()
