"""Build the static browser sender on Windows or Linux."""

import argparse
import shutil
from vft_dev import ROOT, main, run, target_directory


def build(debug=False):
    args = ["cargo", "build", "--locked", "-p", "vft-send-web", "--target", "wasm32-unknown-unknown"]
    if not debug:
        args.append("--release")
    run(*args)
    target = target_directory()
    destination = ROOT / "output" / "web-sender"
    destination.mkdir(parents=True, exist_ok=True)
    # Replace only known distribution assets, preserving unrelated output files.
    for name in ("index.html", "style.css", "app.js", "worker.js", "bridge.js"):
        shutil.copyfile(ROOT / "apps" / "vft-send-web" / "web" / name, destination / name)
    configuration = "debug" if debug else "release"
    shutil.copyfile(target / "wasm32-unknown-unknown" / configuration / "vft_send_web.wasm",
                    destination / "vft_send_web.wasm")
    print(f"Browser sender built: {destination}")
    print("Serve from the repository root with: python -m http.server 18780 "
          "--bind 127.0.0.1 --directory output/web-sender")


def cli():
    parser = argparse.ArgumentParser(description=__doc__, epilog="Install the Rust target first: rustup target add wasm32-unknown-unknown")
    parser.add_argument("--debug", action="store_true", help="build debug instead of release WASM")
    build(parser.parse_args().debug)


if __name__ == "__main__":
    main(cli)
