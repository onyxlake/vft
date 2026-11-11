"""Verify native screen transfers on an unlocked interactive Windows desktop."""

import argparse
from contextlib import ExitStack
import subprocess
import time
from vft_dev import (LoggedProcess, evidence_directory, executable, main, nonnegative,
                     positive, read_stats, require_windows, run, sha256, target_directory,
                     write_csv, write_fixture)


def read_rectangle(path):
    try:
        text = path.read_text(encoding="ascii")
        values = tuple(map(int, text.split()))
    except (FileNotFoundError, UnicodeError, ValueError):
        return None
    if text.endswith("\n") and len(values) == 4 and values[2] > 0 and values[3] > 0:
        return values
    return None


def wait_for_rectangle(path, sender, timeout=15):
    deadline = time.monotonic() + timeout
    while True:
        if sender.poll() is not None:
            raise RuntimeError(f"Sender exited; diagnostics: {path.parent}")
        rectangle = read_rectangle(path)
        if rectangle is not None:
            return rectangle
        if time.monotonic() > deadline:
            raise RuntimeError("Sender window did not publish a complete rectangle in time")
        time.sleep(0.1)


def transfer(args, directory, target, size):
    from vft_windows import ProcessMetrics, move_sender
    input_path = directory / f"{size}-input.bin"
    output_path = directory / f"{size}-recovered.bin"
    output_options = ["--output", output_path]
    receiver_cwd = None
    if args.output_mode != "explicit":
        received = directory / "received"
        received.mkdir(exist_ok=True)
        output_path = received / input_path.name
        output_options = ["--output-dir", received] if args.output_mode == "metadata" else []
        receiver_cwd = received
    rect_path = directory / f"{size}-region.txt"
    stats_path = directory / f"{size}-stats.csv"
    write_fixture(input_path, size)
    capture_mode = "--auto" if args.auto_locate else "--rect" if args.manual_rect else "--sender-window"
    with ExitStack() as stack:
        sender = stack.enter_context(LoggedProcess([
            executable(target, "vft-send-win"), input_path, "--no-tui", "--profile", args.profile,
            "--fps", args.fps, "--cell-size", args.cell_size, "--topmost", "--rect-file", rect_path,
        ], directory, f"{size}-sender"))
        sender_metrics = stack.enter_context(ProcessMetrics(sender))
        wait_for_rectangle(rect_path, sender)
        time.sleep(args.join_delay_seconds)
        capture = [capture_mode]
        if args.manual_rect:
            capture.extend(wait_for_rectangle(rect_path, sender))
        receiver = stack.enter_context(LoggedProcess([
            executable(target, "vft-recv-win"), *capture, "--no-tui", *output_options,
            "--fps", "120", "--timeout", args.timeout_seconds, "--stats", stats_path,
            "--preview", directory / f"{size}-preview.bmp",
        ], directory, f"{size}-receiver", cwd=receiver_cwd))
        receiver_metrics = stack.enter_context(ProcessMetrics(receiver))
        started = time.monotonic()
        moved = False
        while True:
            try:
                receiver.wait(timeout=1)
                break
            except subprocess.TimeoutExpired:
                if sender.poll() is not None:
                    raise RuntimeError("Sender exited during capture")
                receiver_metrics.sample()
                if args.move_sender and not moved and time.monotonic() - started >= 3:
                    log = (directory / f"{size}-receiver.log").read_text(encoding="utf-8", errors="replace")
                    if any(line.startswith("LOCATED ") for line in log.splitlines()):
                        move_sender(sender)
                        moved = True
                if time.monotonic() - started > args.timeout_seconds + 30:
                    raise RuntimeError(f"Receiver exceeded timeout; diagnostics: {directory}")
        if receiver.returncode != 0:
            raise RuntimeError(f"Receiver failed; diagnostics: {directory}")
        actual = sha256(output_path)
        if sha256(input_path) != actual:
            raise RuntimeError("Output SHA-256 does not match input")
        stats = read_stats(stats_path)
        locks = int(stats["location_locks"])
        if args.auto_locate and locks < 1:
            raise RuntimeError("Auto mode did not locate a frame")
        if args.move_sender and (not moved or locks < 2):
            raise RuntimeError("Movement test did not reacquire the sender")
        stats.update(sender_metrics.values("sender"))
        stats.update(receiver_metrics.values("receiver"))
        stats.update(sha256=actual, requested_fps=args.fps, cell_size=args.cell_size,
                     join_delay_seconds=args.join_delay_seconds, capture_mode=capture_mode,
                     sender_moved=moved, capture_region=" ".join(map(str, wait_for_rectangle(rect_path, sender))),
                     output_mode=args.output_mode)
        print(f"PASS size={size} SHA256={actual} goodput={stats['verified_goodput_bytes_per_second']} B/s")
        return stats


def check():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--sizes", type=nonnegative, nargs="+", default=[1024, 1048576, 104857600], help="space-separated file sizes in bytes")
    parser.add_argument("--profile", choices=["robust", "balanced"], default="balanced")
    parser.add_argument("--fps", type=positive, default=30)
    parser.add_argument("--cell-size", type=positive, default=4)
    parser.add_argument("--timeout-seconds", type=positive, default=3600)
    parser.add_argument("--join-delay-seconds", type=nonnegative, default=0)
    capture = parser.add_mutually_exclusive_group()
    capture.add_argument("--auto-locate", action="store_true")
    capture.add_argument("--manual-rect", action="store_true")
    parser.add_argument("--move-sender", action="store_true")
    parser.add_argument("--skip-build", action="store_true")
    parser.add_argument("--output-mode", choices=["explicit", "metadata", "current-dir"], default="explicit",
                        help="receiver destination: explicit file, --output-dir with metadata name, or default working directory")
    args = parser.parse_args()
    if args.move_sender and not args.auto_locate:
        parser.error("--move-sender requires --auto-locate")
    if len(set(args.sizes)) != len(args.sizes):
        parser.error("--sizes must not contain duplicates")
    require_windows()
    if not args.skip_build:
        run("cargo", "build", "--release", "--workspace")
    target = target_directory()
    directory = evidence_directory("local")
    results = []
    for size in args.sizes:
        results.append(transfer(args, directory, target, size))
        # Preserve already verified cases even if a later case fails.
        write_csv(directory / f"{size}-result.csv", [results[-1]])
    write_csv(directory / "results.csv", results)
    print(f"Results: {directory}")


if __name__ == "__main__":
    main(check)
