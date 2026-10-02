"""Check compiled WASM/native recovery (Rust WASM target and Node.js 22+ required)."""

import argparse
import sys
from vft_dev import ROOT, main, run, target_directory


def check():
    argparse.ArgumentParser(description=__doc__).parse_args()
    run(sys.executable, ROOT / "scripts" / "build-web.py")
    run("cargo", "clippy", "-p", "vft-send-web", "--target", "wasm32-unknown-unknown", "--", "-D", "warnings")
    for name in ("app.js", "bridge.js", "worker.js"):
        run("node", "--check", ROOT / "apps" / "vft-send-web" / "web" / name)
    run("cargo", "build", "-p", "vft-send-web", "--example", "verify-frames")
    verifier = target_directory() / "debug" / "examples" / ("verify-frames.exe" if sys.platform == "win32" else "verify-frames")
    run("node", ROOT / "scripts" / "web-e2e.mjs", verifier)


if __name__ == "__main__":
    main(check)
