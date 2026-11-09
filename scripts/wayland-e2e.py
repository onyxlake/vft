"""Interactive Linux Wayland transfer check; opens a sender and ScreenCast portal picker."""

import argparse
from contextlib import ExitStack
import os
import subprocess
import sys
import time
from vft_dev import (LoggedProcess, evidence_directory, executable, main, nonnegative,
                     positive, read_stats, run, sha256, target_directory, write_csv, write_fixture)


def require_wayland():
    if sys.platform != "linux" or not os.environ.get("WAYLAND_DISPLAY"):
        raise RuntimeError("This check requires an interactive Linux Wayland desktop and ScreenCast portal.")


def transfer(args, directory, target, size):
    source = directory / f"{size}-input.bin"
    output = directory / f"{size}-recovered.bin"
    output_options = ["--output", output]
    receiver_cwd = None
    if args.output_mode != "explicit":
        received = directory / "received"
        received.mkdir(exist_ok=True)
        output = received / source.name
        output_options = ["--output-dir", received] if args.output_mode == "metadata" else []
        receiver_cwd = received
    stats = directory / f"{size}-stats.csv"
    write_fixture(source, size)
    with ExitStack() as stack:
        sender = stack.enter_context(LoggedProcess([
            executable(target, "vft-send-linux"), source, "--no-tui", "--profile", args.profile,
            "--fps", args.fps, "--cell-size", args.cell_size,
        ], directory, f"{size}-sender"))
        time.sleep(args.join_delay_seconds)
        if sender.poll() is not None:
            raise RuntimeError(f"Sender exited; diagnostics: {directory}")
        print("In the portal, select the VFT sender window or its monitor. Keep the frame visible.", flush=True)
        receiver = stack.enter_context(LoggedProcess([
            executable(target, "vft-recv-linux"), "--auto", "--no-tui", *output_options,
            "--fps", "60", "--timeout", args.timeout_seconds, "--stats", stats,
            "--preview", directory / f"{size}-preview.bmp",
        ], directory, f"{size}-receiver", cwd=receiver_cwd))
        started = time.monotonic()
        while True:
            try:
                receiver.wait(timeout=1)
                break
            except subprocess.TimeoutExpired:
                if sender.poll() is not None:
                    raise RuntimeError(f"Sender exited during capture; diagnostics: {directory}")
                if time.monotonic() - started > args.timeout_seconds + 15:
                    raise RuntimeError(f"Receiver exceeded timeout; diagnostics: {directory}")
        if receiver.returncode != 0:
            raise RuntimeError(f"Receiver failed; diagnostics: {directory}")
        digest = sha256(output)
        if sha256(source) != digest:
            raise RuntimeError("Independent input/output SHA-256 mismatch")
        row = read_stats(stats)
        if row["success"] != "true" or int(row["location_locks"]) < 1:
            raise RuntimeError("Receiver did not record verified auto-location success")
        row.update(sha256=digest, profile=args.profile, cell_size=args.cell_size,
                   sender_requested_fps=args.fps, join_delay_seconds=args.join_delay_seconds,
                   transport="local-wayland", observed_display_fps="", cpu="", memory="",
                   output_mode=args.output_mode)
        print(f"PASS size={size} SHA256={digest} goodput={row['verified_bytes_per_second']} B/s", flush=True)
        return row


def check():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--sizes", type=nonnegative, nargs="+", default=[1024, 1048576])
    parser.add_argument("--profile", choices=["robust", "balanced"], default="balanced")
    parser.add_argument("--fps", type=positive, default=30)
    parser.add_argument("--cell-size", type=positive, default=4)
    parser.add_argument("--timeout-seconds", type=positive, default=300)
    parser.add_argument("--join-delay-seconds", type=nonnegative, default=1)
    parser.add_argument("--skip-build", action="store_true")
    parser.add_argument("--output-mode", choices=["explicit", "metadata", "current-dir"], default="explicit",
                        help="receiver destination: explicit file, --output-dir with metadata name, or default working directory")
    args = parser.parse_args()
    if len(set(args.sizes)) != len(args.sizes):
        parser.error("--sizes must not contain duplicates")
    if args.fps > 60 or not 2 <= args.cell_size <= 32:
        parser.error("--fps must be 1..60 and --cell-size 2..32")
    require_wayland()
    if not args.skip_build:
        run("cargo", "build", "--release", "-p", "vft-send-linux", "-p", "vft-recv-linux")
    target = target_directory()
    directory = evidence_directory("local-wayland")
    rows = []
    for size in args.sizes:
        rows.append(transfer(args, directory, target, size))
        write_csv(directory / f"{size}-verified.csv", [rows[-1]])
    write_csv(directory / "summary.csv", rows)


if __name__ == "__main__":
    main(check)
