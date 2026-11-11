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
from unittest.mock import patch

import vft_dev


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
                patch.object(build, "run") as command:
            build.build()
            self.assertIn("--release", command.call_args.args)
            self.assertEqual(embedded_wasm(), b"release")
            build.build(debug=True)
            self.assertNotIn("--release", command.call_args.args)
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
        html = build.bundle(assets, b"\0asm\xff")
        worker = re.search(r'id="vft-worker"[^>]*>(.*?)</script>', html).group(1)
        self.assertNotIn("<", worker)
        self.assertTrue(json.loads(worker).endswith(delimiter))
        original = (assets / "index.html").read_text(encoding="utf-8")
        for replacement in ("", "<!-- VFT_APP --><!-- VFT_APP -->"):
            (assets / "index.html").write_text(original.replace("<!-- VFT_APP -->", replacement), encoding="utf-8")
            with self.assertRaisesRegex(ValueError, "exactly one web bundle marker"):
                build.bundle(assets, b"wasm")
        (assets / "index.html").write_text(original, encoding="utf-8")
        (assets / "bridge.js").write_text("export async function renamed() {}", encoding="utf-8")
        with self.assertRaisesRegex(ValueError, "exactly one web bundle marker"):
            build.bundle(assets, b"wasm")

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
