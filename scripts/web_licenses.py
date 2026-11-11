"""Collect offline redistribution notices for the locked WASM sender build."""

from html.parser import HTMLParser
import json
import os
from pathlib import Path
import re
import subprocess
import sys


def capture(root, *args):
    try:
        return subprocess.run(list(args), cwd=root, check=True, capture_output=True,
                              text=True, encoding="utf-8").stdout.strip()
    except subprocess.CalledProcessError as error:
        if error.stderr:
            print(error.stderr, file=sys.stderr, end="")
        raise


def read_notice(path):
    text = path.read_text(encoding="utf-8").strip()
    if not text:
        raise ValueError(f"Empty license notice: {path}")
    return text


def dependency_notices(metadata):
    """Walk normal/build dependencies in Cargo's platform-filtered graph, excluding dev edges."""
    packages = {package["id"]: package for package in metadata["packages"]}
    nodes = {node["id"]: node for node in metadata["resolve"]["nodes"]}
    workspace = set(metadata["workspace_members"])
    roots = [key for key in workspace if packages[key]["name"] == "vft-send-web"]
    if len(roots) != 1:
        raise ValueError("Expected one vft-send-web workspace package")
    pending, seen = roots, set()
    while pending:
        key = pending.pop()
        if key in seen:
            continue
        seen.add(key)
        pending.extend(dep["pkg"] for dep in nodes[key]["deps"]
                       if any(kind["kind"] != "dev" for kind in dep["dep_kinds"]))
    notices = []
    for package in sorted((packages[key] for key in seen), key=lambda p: (p["name"], p["version"])):
        name = f'{package["name"]} {package["version"]}'
        if package["id"] in workspace:
            if package["license"] != "MIT":
                raise ValueError(f"Review changed workspace license: {name}")
            continue  # These packages are covered by the root VFT license.
        license_id = package["license"]
        if license_id not in ("MIT", "MIT OR Apache-2.0", "Apache-2.0 OR MIT", "MIT/Apache-2.0"):
            raise ValueError(f"Review unsupported web dependency license: {name}: {license_id}")
        directory = Path(package["manifest_path"]).parent
        names = ("LICENSE-MIT", "LICENSE-MIT.txt")
        if license_id == "MIT":
            names += ("LICENSE",)
        path = next((directory / name for name in names if (directory / name).is_file()), None)
        if path is None:
            raise ValueError(f"Missing MIT license text for web dependency: {name}")
        text = read_notice(path)
        if "Permission is hereby granted" not in text or "copyright" not in text.lower():
            raise ValueError(f"Review unexpected MIT license text: {path}")
        section = f"{name} — MIT\n\n{text}"
        for filename in ("NOTICE", "NOTICE.txt", "NOTICE.md"):
            extra = directory / filename
            if extra.is_file():
                section += f"\n\n{filename}\n{read_notice(extra)}"
        notices.append(section)
    return notices


class CopyrightText(HTMLParser):
    """Keep the complete notice body as text, without embedding upstream HTML."""

    def __init__(self):
        super().__init__(convert_charrefs=True)
        self.in_body = False
        self.parts = []

    def handle_starttag(self, tag, attrs):
        if tag == "body":
            self.in_body = True
        if self.in_body and tag in ("p", "div", "pre", "li", "h1", "h2", "h3", "br"):
            self.parts.append("\n")

    def handle_endtag(self, tag):
        if tag == "body":
            self.in_body = False
        elif self.in_body and tag in ("p", "div", "pre", "li", "h1", "h2", "h3"):
            self.parts.append("\n")

    def handle_data(self, data):
        if self.in_body:
            self.parts.append(data)


def toolchain_notices(sysroot, version):
    directory = sysroot / "share/doc/rust"
    names = ("COPYRIGHT-library.html", "COPYRIGHT", "COPYRIGHT.html")
    path = next((directory / name for name in names if (directory / name).is_file()), None)
    if path is None:
        raise ValueError(f"Rust copyright notices are missing from {directory}")
    text = read_notice(path)
    if path.suffix == ".html":
        parser = CopyrightText()
        parser.feed(text)
        parser.close()
        text = re.sub(r"\n[ \t]*(?:\n[ \t]*)+", "\n\n", "".join(parser.parts)).strip()
        if not text:
            raise ValueError(f"Rust copyright notice has no HTML body: {path}")
    # Rust releases use either a licenses directory or top-level LICENSE-* files.
    licenses = sorted((directory / "licenses").glob("*.txt"))
    if not licenses:
        licenses = sorted(directory.glob("LICENSE-*"))
    if not licenses:
        raise ValueError(f"Rust license texts are missing from {directory}")
    sections = [f"Rust runtime — {version}\n\n"
                "The toolchain's supplied notices also cover other platforms and components; "
                "their inclusion does not mean every component is linked into this WASM file.\n\n"
                f"{path.name}\n\n{text}"]
    sections.extend(f"Rust {path.name}\n\n{read_notice(path)}" for path in licenses)
    return sections


def collect_notices(root):
    metadata = json.loads(capture(root, "cargo", "metadata", "--locked", "--offline",
                                  "--format-version", "1", "--filter-platform", "wasm32-unknown-unknown"))
    rustc = os.environ.get("RUSTC", "rustc")
    sysroot = Path(capture(root, rustc, "--print", "sysroot"))
    version = capture(root, rustc, "--version")
    sections = ["VFT browser sender — MIT\n\n" + read_notice(root / "LICENSE")]
    sections.extend(dependency_notices(metadata))
    sections.extend(toolchain_notices(sysroot, version))
    return ("Licenses and third-party notices\n\n"
            "VFT uses the MIT option for dual-licensed Cargo dependencies listed below.\n\n"
            + ("\n\n" + "=" * 72 + "\n\n").join(sections))
