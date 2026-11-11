"""Regression checks for release names, binary architectures and ZIP contents."""

import importlib.util
import json
from pathlib import Path
import struct
import tempfile
import unittest
from unittest.mock import patch
import zipfile

import vft_dev


spec = importlib.util.spec_from_file_location("package_native", vft_dev.ROOT / "scripts/package-native.py")
release = importlib.util.module_from_spec(spec)
spec.loader.exec_module(release)


def executable_header(platform, arch):
    if platform == "windows":
        header = bytearray(134)
        header[:2] = b"MZ"
        struct.pack_into("<I", header, 60, 128)
        header[128:132] = b"PE\0\0"
        struct.pack_into("<H", header, 132, 0x14C if arch == "386" else 0x8664)
    else:
        header = bytearray(64)
        header[:6] = b"\x7fELF" + bytes((1 if arch == "386" else 2, 1))
        struct.pack_into("<H", header, 18, 3 if arch == "386" else 62)
    return bytes(header)


class ReleasePackages(unittest.TestCase):
    def setUp(self):
        output = vft_dev.ROOT / "output"
        output.mkdir(exist_ok=True)
        temporary = tempfile.TemporaryDirectory(prefix="release-tests-", dir=output)
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)

    def test_release_and_development_names_and_package_version_ordering(self):
        self.assertEqual(release.build_version("0.1.0", {"GITHUB_REF_TYPE": "tag", "GITHUB_REF_NAME": "v1.2.3"}), "v1.2.3")
        self.assertEqual(release.build_version("1.2.3", {"GITHUB_SHA": "ab" * 20}), "v1.2.3-dev.abababababab")
        self.assertEqual(release.build_version("1.2.3-rc.1", {"GITHUB_SHA": "ab" * 20}), "v1.2.3-rc.1.dev.abababababab")
        self.assertEqual(release.package_versions("v1.2.3"), ("1.2.3-1", "1.2.3"))
        self.assertEqual(release.package_versions("v1.2.3-rc.1+build.7"), ("1.2.3~rc.1-1", "1.2.3~rc.1"))
        for invalid in ("v1.2", "v01.2.3", "v1.2.3-01", "../../release", "v1.2.3\nName: other", "v1.2.3;echo"):
            with self.subTest(version=invalid), self.assertRaises(ValueError):
                release.version_parts(invalid)

    def test_binary_headers_must_match_platform_and_architecture(self):
        for target, (platform, arch) in release.TARGETS.items():
            binary = self.root / target
            binary.write_bytes(executable_header(platform, arch))
            release.verify_binary(binary, target)
            for other in release.TARGETS:
                if other != target:
                    with self.subTest(target=target, other=other), self.assertRaises(ValueError):
                        release.verify_binary(binary, other)
            binary.write_bytes(b"MZ")
            with self.assertRaises(ValueError):
                release.verify_binary(binary, target)
        binary.write_bytes(b"MZ" + b"\0" * 58 + b"\xff" * 4)
        with self.assertRaises(ValueError):
            release.verify_binary(binary, "i686-pc-windows-msvc")

    def test_zip_contains_only_binaries_preserves_bytes_mode_and_existing_archive(self):
        binaries = []
        for kind in ("send", "recv"):
            path = self.root / f"vft-{kind}-linux"
            path.write_bytes(executable_header("linux", "386") + kind.encode())
            path.chmod(0o755)
            binaries.append(path)
        (self.root / "README.md").write_text("Must not be included")
        (self.root / "LICENSE").write_text("Must not be included in ZIPs")
        destination = self.root / "vft-v0.1.0-linux-386.zip"
        release.write_zip(binaries, destination)
        original = destination.read_bytes()
        with zipfile.ZipFile(destination) as archive:
            self.assertEqual(archive.namelist(), [path.name for path in binaries])
            for path in binaries:
                self.assertEqual(archive.read(path.name), path.read_bytes())
                # Unix ZIP metadata preserves execution rights on Linux builds.
                if archive.getinfo(path.name).create_system == 3:
                    self.assertEqual((archive.getinfo(path.name).external_attr >> 16) & 0o777, 0o755)
        with self.assertRaises(FileExistsError):
            release.write_zip(binaries, destination)
        self.assertEqual(destination.read_bytes(), original)

    def test_packaging_uses_explicit_target_directory_and_versioned_name(self):
        target = "i686-pc-windows-msvc"
        directory = self.root / "custom-target" / target / "release"
        directory.mkdir(parents=True)
        for kind in ("send", "recv"):
            (directory / f"vft-{kind}-win.exe").write_bytes(executable_header("windows", "386"))
        metadata = {"target_directory": str(self.root / "custom-target"),
                    "packages": [{"name": "vft-protocol", "version": "0.1.0"}]}
        with patch.object(release, "ROOT", self.root), patch.object(release, "command", return_value=json.dumps(metadata)):
            release.package(target, "v1.2.3")
            archive = self.root / "output/release/vft-v1.2.3-windows-386.zip"
            self.assertTrue(archive.is_file())
            with self.assertRaises(FileExistsError):
                release.package(target, "v1.2.3")
            release.package(target, "v1.2.3", Path("output/custom"))
            self.assertTrue((self.root / "output/custom" / archive.name).is_file())
            with self.assertRaises(ValueError):
                release.package(target, "v1.2.3", self.root / "outside")
            # Reject a mislabeled 64-bit sender before writing any new archive.
            (directory / "vft-send-win.exe").write_bytes(executable_header("windows", "amd64"))
            with self.assertRaises(ValueError):
                release.package(target, "v2.0.0")
            self.assertFalse((archive.parent / "vft-v2.0.0-windows-386.zip").exists())


if __name__ == "__main__":
    unittest.main()
