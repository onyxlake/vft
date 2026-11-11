"""Build a self-contained browser sender index.html on Windows or Linux."""

import argparse
import base64
import json
import re
from vft_dev import ROOT, main, run, target_directory


def replace_once(source, marker, replacement):
    if source.count(marker) != 1:
        raise ValueError(f"Expected exactly one web bundle marker: {marker}")
    return source.replace(marker, replacement)


def bundle(assets, wasm):
    """Join this small, fixed module graph; fail if its declared boundaries change."""
    read = lambda name: (assets / name).read_text(encoding="utf-8")
    bridge = replace_once(read("bridge.js"), "export async function instantiateSender(",
                          "async function instantiateSender(")
    worker = replace_once(read("worker.js"), 'import { instantiateSender } from "./bridge.js";\n', "")
    # JSON escapes prevent HTML raw-text delimiters in the Worker from ending its script tag.
    worker_json = json.dumps(bridge + "\n" + worker, ensure_ascii=True).replace("<", "\\u003c")
    style, app = read("style.css"), read("app.js")
    if re.search(r"</style", style, re.IGNORECASE) or re.search(r"</script|<script|<!--", app, re.IGNORECASE):
        raise ValueError("Unexpected HTML raw-text delimiter in inline CSS or app JavaScript")
    html = read("index.html")
    for name, content in (
        ("STYLE", f"<style>\n{style}</style>"),
        ("WORKER", f'<script id="vft-worker" type="application/json">{worker_json}</script>'),
        ("WASM", '<script id="vft-wasm" type="application/octet-stream">'
         + base64.b64encode(wasm).decode("ascii") + '</script>'),
        ("APP", f"<script>\n(() => {{\n{app}\n}})();\n</script>"),
    ):
        html = replace_once(html, f"<!-- VFT_{name} -->", content)
    return html


def build(debug=False):
    args = ["cargo", "build", "--locked", "-p", "vft-send-web", "--target", "wasm32-unknown-unknown"]
    if not debug:
        args.append("--release")
    run(*args)
    target = target_directory()
    configuration = "debug" if debug else "release"
    wasm = (target / "wasm32-unknown-unknown" / configuration / "vft_send_web.wasm").read_bytes()
    html = bundle(ROOT / "apps" / "vft-send-web" / "web", wasm)
    destination = ROOT / "output" / "web-sender"
    destination.mkdir(parents=True, exist_ok=True)
    (destination / "index.html").write_text(html, encoding="utf-8")
    # Remove only obsolete generated assets, preserving unrelated output files.
    for name in ("style.css", "app.js", "worker.js", "bridge.js", "vft_send_web.wasm"):
        (destination / name).unlink(missing_ok=True)
    print(f"Browser sender built: {destination / 'index.html'}")
    print("Open index.html directly in your browser (file://); no server is required.")


def cli():
    parser = argparse.ArgumentParser(description=__doc__, epilog="Install the Rust target first: rustup target add wasm32-unknown-unknown")
    parser.add_argument("--debug", action="store_true", help="build debug instead of release WASM")
    build(parser.parse_args().debug)


if __name__ == "__main__":
    main(cli)
