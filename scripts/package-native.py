"""Package explicit Rust targets as binary-only ZIPs and Linux DEB/RPM files."""

import argparse
import json
import os
from pathlib import Path
import re
import shlex
import shutil
import struct
import subprocess
import tempfile
import zipfile

from vft_dev import ROOT, main


TARGETS = {
    "i686-pc-windows-msvc": ("windows", "386"),
    "x86_64-pc-windows-msvc": ("windows", "amd64"),
    "i686-unknown-linux-gnu": ("linux", "386"),
    "x86_64-unknown-linux-gnu": ("linux", "amd64"),
}
VERSION = re.compile(
    r"v?(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)"
    r"(?:-([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?"
    r"(?:\+([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?"
)


def command(*args, cwd=ROOT, capture=False, env=None):
    print("+ " + " ".join(map(str, args)), flush=True)
    return subprocess.run(
        list(map(str, args)), cwd=cwd, check=True, env=env,
        stdout=subprocess.PIPE if capture else None, text=True, encoding="utf-8",
    ).stdout


def version_parts(version):
    match = VERSION.fullmatch(version)
    if match is None:
        raise ValueError("Version must be a semantic version, optionally prefixed with v")
    major, minor, patch, prerelease, metadata = match.groups()
    if prerelease and any(part.isdigit() and len(part) > 1 and part.startswith("0")
                          for part in prerelease.split(".")):
        raise ValueError("Numeric prerelease identifiers must not have leading zeroes")
    return f"{major}.{minor}.{patch}", prerelease, metadata


def build_version(workspace_version, environment):
    if environment.get("GITHUB_REF_TYPE") == "tag":
        return environment["GITHUB_REF_NAME"]
    sha = environment.get("GITHUB_SHA")
    if not sha:
        return "v" + workspace_version
    if not re.fullmatch(r"[0-9a-f]{40,64}", sha):
        raise ValueError("Invalid build commit SHA")
    base, prerelease, _ = version_parts(workspace_version)
    return f"v{base}-{prerelease + '.' if prerelease else ''}dev.{sha[:12]}"


def package_versions(version):
    base, prerelease, _ = version_parts(version)
    # A prerelease must sort before the corresponding stable package. SemVer build
    # metadata does not affect precedence and is retained only in download names.
    deb = base + ("~" + prerelease if prerelease else "") + "-1"
    rpm = base + ("~" + prerelease.replace("-", ".") if prerelease else "")
    return deb, rpm


def verify_binary(path, target):
    """Reject host stubs and accidentally mislabeled architectures before packaging."""
    platform, arch = TARGETS[target]
    with path.open("rb") as stream:
        header = stream.read(64)
        if platform == "windows":
            if len(header) != 64 or header[:2] != b"MZ":
                raise ValueError(f"Expected a Windows PE executable: {path}")
            offset = struct.unpack_from("<I", header, 60)[0]
            if offset < 64 or offset > path.stat().st_size - 6:
                raise ValueError(f"Invalid PE header offset: {path}")
            stream.seek(offset)
            pe = stream.read(6)
            expected = 0x14C if arch == "386" else 0x8664
            valid = len(pe) == 6 and pe[:4] == b"PE\0\0" and struct.unpack_from("<H", pe, 4)[0] == expected
        else:
            expected_class, expected_machine = (1, 3) if arch == "386" else (2, 62)
            valid = (len(header) >= 20 and header[:4] == b"\x7fELF"
                     and header[4:6] == bytes((expected_class, 1))
                     and struct.unpack_from("<H", header, 18)[0] == expected_machine)
        if not valid:
            raise ValueError(f"Executable does not match {target}: {path}")


def copy_new(source, destination, mode=None):
    destination.parent.mkdir(parents=True, exist_ok=True)
    with source.open("rb") as input_file, destination.open("xb") as output:
        shutil.copyfileobj(input_file, output)
    if mode is not None:
        destination.chmod(mode)


def write_zip(binaries, destination):
    with zipfile.ZipFile(destination, "x", zipfile.ZIP_DEFLATED) as archive:
        for path in binaries:
            archive.write(path, path.name)


def stage_linux(binaries, directory, license_path):
    for path in binaries:
        copy_new(path, directory / "usr/bin" / path.name, 0o755)
    copy_new(ROOT / "LICENSE", directory / license_path, 0o644)


def linux_packages(binaries, version, arch, work, destinations):
    deb_arch, rpm_arch, multiarch = (("i386", "i686", "i386-linux-gnu") if arch == "386"
                                     else ("amd64", "x86_64", "x86_64-linux-gnu"))
    deb_version, rpm_version = package_versions(version)
    deb_root = work / "debian/vft"
    stage_linux(binaries, deb_root, "usr/share/doc/vft/copyright")
    (work / "debian/control").write_text(
        "Source: vft\nMaintainer: VFT contributors\n\n"
        "Package: vft\nArchitecture: any\nDescription: Visual file transfer sender and receiver\n",
        encoding="utf-8",
    )
    environment = dict(os.environ, DEB_HOST_ARCH=deb_arch, DEB_HOST_MULTIARCH=multiarch)
    dependencies = command(
        "dpkg-shlibdeps", "-O", f"-l/usr/lib/{multiarch}",
        *(f"-edebian/vft/usr/bin/{path.name}" for path in binaries), cwd=work, capture=True, env=environment,
    ).strip()
    if not dependencies.startswith("shlibs:Depends=") or "\n" in dependencies:
        raise ValueError(f"Unexpected shared-library dependency output: {dependencies}")
    dependencies = dependencies.removeprefix("shlibs:Depends=")
    # Wayland and xkbcommon are loaded dynamically, so ELF dependency scanning
    # cannot discover them. The portal backend is selected by the user's desktop.
    dependencies += ", libwayland-client0, libxkbcommon0, xdg-desktop-portal"
    control = deb_root / "DEBIAN/control"
    control.parent.mkdir()
    control.write_text(
        f"Package: vft\nVersion: {deb_version}\nArchitecture: {deb_arch}\n"
        "Maintainer: VFT contributors\nSection: utils\nPriority: optional\n"
        f"Depends: {dependencies}\nRecommends: pipewire\n"
        "Description: Visual file transfer sender and receiver\n"
        " Transfers files through independent visual frames in a Wayland session.\n"
        " Capture requires a configured ScreenCast portal backend.\n",
        encoding="utf-8",
    )
    deb = work / "vft.deb"
    command("dpkg-deb", "--root-owner-group", "-Zxz", "--build", deb_root, deb)
    if command("dpkg-deb", "--field", deb, "Architecture", capture=True).strip() != deb_arch:
        raise ValueError("DEB architecture mismatch")

    rpm_root = work / "rpm"
    payload = rpm_root / "payload"
    stage_linux(binaries, payload, "usr/share/licenses/vft/LICENSE")
    buildroot = rpm_root / "buildroot"
    # rpmbuild may recursively clean its buildroot; it must be inside this run's
    # newly-created staging directory, never a user-supplied or repository path.
    buildroot.resolve().relative_to(work.resolve())
    quote = lambda path: shlex.quote(str(path)).replace("%", "%%")
    capability_suffix = "()(64bit)" if arch == "amd64" else ""
    spec = rpm_root / "vft.spec"
    spec.write_text(
        # Rust has already built and checked the executables. Keep those exact
        # bytes instead of generating debug subpackages or stripping them again.
        "%global debug_package %{nil}\n%global __os_install_post %{nil}\n"
        "%global _build_id_links none\n"
        # --target sets the architecture for these prebuilt binaries. BuildArch
        # would instead ask RPM to select a compatible build-host architecture.
        f"Name: vft\nVersion: {rpm_version}\nRelease: 1\n"
        "Summary: Visual file transfer sender and receiver\nLicense: MIT\n"
        "AutoReqProv: yes\n"
        f"Requires: libwayland-client.so.0{capability_suffix}\n"
        f"Requires: libxkbcommon.so.0{capability_suffix}\n"
        "Requires: xdg-desktop-portal\nRecommends: pipewire\n\n"
        "%description\nTransfers files through independent visual frames in a Wayland session.\n"
        "Capture requires a configured ScreenCast portal backend.\n\n"
        f"%install\nmkdir -p {quote(buildroot)}\ncp -a {quote(str(payload) + '/.')} {quote(buildroot)}\n\n"
        "%files\n%defattr(-,root,root,-)\n/usr/bin/vft-send-linux\n/usr/bin/vft-recv-linux\n"
        "%license /usr/share/licenses/vft/LICENSE\n",
        encoding="utf-8",
    )
    command("rpmbuild", "-bb", "--target", rpm_arch, "--buildroot", buildroot,
            "--define", f"_topdir {rpm_root}", spec)
    rpms = list((rpm_root / "RPMS").glob("*/*.rpm"))
    if len(rpms) != 1:
        raise ValueError("Expected exactly one binary RPM")
    if command("rpm", "-qp", "--queryformat", "%{ARCH}", rpms[0], capture=True).strip() != rpm_arch:
        raise ValueError("RPM architecture mismatch")
    for source, destination in zip((deb, rpms[0]), destinations):
        copy_new(source, destination)


def package(target, version=None, output_dir=None):
    metadata = json.loads(command("cargo", "metadata", "--locked", "--no-deps",
                                  "--format-version", "1", capture=True))
    platform, arch = TARGETS[target]
    suffix = "-win.exe" if platform == "windows" else "-linux"
    workspace_version = next(p["version"] for p in metadata["packages"] if p["name"] == "vft-protocol")
    version = version or build_version(workspace_version, os.environ)
    version_parts(version)
    version = "v" + version.removeprefix("v")
    destination = output_dir or Path("output/release")
    if not destination.is_absolute():
        destination = ROOT / destination
    destination = destination.resolve()
    destination.relative_to((ROOT / "output").resolve())
    binaries = [Path(metadata["target_directory"]) / target / "release" / f"vft-{kind}{suffix}"
                for kind in ("send", "recv")]
    for path in binaries:
        verify_binary(path, target)
    formats = ("zip", "deb", "rpm") if platform == "linux" else ("zip",)
    paths = [destination / f"vft-{version}-{platform}-{arch}.{extension}" for extension in formats]
    for path in paths:
        if path.exists() or path.is_symlink():
            raise FileExistsError(path)
    if platform == "linux":
        for tool in ("dpkg-deb", "dpkg-shlibdeps", "rpmbuild", "rpm"):
            if shutil.which(tool) is None:
                raise RuntimeError(f"Linux packaging requires {tool} (install dpkg-dev and rpm)")
    destination.mkdir(parents=True, exist_ok=True)
    write_zip(binaries, paths[0])
    if platform == "linux":
        work = Path(tempfile.mkdtemp(prefix="package-native-", dir=ROOT / "output"))
        print(f"Packaging evidence: {work}", flush=True)
        linux_packages(binaries, version, arch, work, paths[1:])
    for path in paths:
        print(f"Packaged: {path}")


def cli():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--target", required=True, choices=TARGETS)
    parser.add_argument("--version", help="release tag; defaults to the GitHub tag or workspace version plus build commit")
    parser.add_argument("--output-dir", type=Path, help="new artifacts directory under output/ (default: output/release)")
    args = parser.parse_args()
    package(args.target, args.version, args.output_dir)


if __name__ == "__main__":
    main(cli)
