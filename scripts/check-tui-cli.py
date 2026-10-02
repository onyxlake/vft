"""Probe redirected CLI/TUI modes on an interactive Windows desktop."""

import argparse
import subprocess
from vft_dev import LoggedProcess, evidence_directory, executable, main, require_windows, run, target_directory


def probe(directory, target, name, app, arguments, expected, message=""):
    with LoggedProcess([executable(target, app), *arguments], directory, name) as process:
        try:
            code = process.wait(timeout=15)
        except subprocess.TimeoutExpired as error:
            raise RuntimeError(f"{name} timed out") from error
        if code != expected:
            raise RuntimeError(f"{name} exit code {code}, expected {expected}; diagnostics: {directory}")
    text = "".join((directory / f"{name}.{suffix}").read_text(encoding="utf-8", errors="replace")
                   for suffix in ("log", "err"))
    if "\x1b" in text:
        raise RuntimeError(f"{name} wrote terminal escapes to redirected output")
    if message and message not in text:
        raise RuntimeError(f"{name} missing expected English context: {message}")
    print(f"PASS {name}")


def check():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--skip-build", action="store_true")
    args = parser.parse_args()
    require_windows()
    if not args.skip_build:
        run("cargo", "build", "--release", "--workspace")
    target = target_directory()
    directory = evidence_directory("tui-cli")
    pattern = ["--test-pattern", "--cell-size", "2", "--fps", "60", "--frames", "2"]
    probe(directory, target, "sender-default-redirected", "vft-send-win", pattern, 0)
    probe(directory, target, "sender-no-tui", "vft-send-win", [*pattern, "--no-tui"], 0)
    interactive = "TUI requires interactive stdin and stdout"
    probe(directory, target, "sender-explicit-tui-redirected", "vft-send-win", ["--test-pattern", "--tui"], 1, interactive)
    probe(directory, target, "sender-conflicting-modes", "vft-send-win", ["--test-pattern", "--tui", "--no-tui"], 1, "mutually exclusive")
    output = ["--auto", "--output", str(directory / "unused.bin")]
    probe(directory, target, "receiver-explicit-tui-redirected", "vft-recv-win", [*output, "--tui"], 1, interactive)
    probe(directory, target, "receiver-conflicting-modes", "vft-recv-win", [*output, "--tui", "--no-tui"], 1, "mutually exclusive")


if __name__ == "__main__":
    main(check)
