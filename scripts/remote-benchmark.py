"""Record a remote transfer captured by the native Windows receiver."""

import argparse
from datetime import datetime, timezone
import json
import subprocess
import time
from vft_dev import (LoggedProcess, evidence_directory, executable, main, positive,
                     read_stats, require_windows, sha256, target_directory, write_csv)


def check():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--x", type=int, required=True)
    parser.add_argument("--y", type=int, required=True)
    parser.add_argument("--width", type=positive, required=True)
    parser.add_argument("--height", type=positive, required=True)
    parser.add_argument("--profile", choices=["robust", "balanced"], required=True)
    parser.add_argument("--sender-fps", type=positive, required=True)
    parser.add_argument("--cell-size", type=positive, required=True)
    parser.add_argument("--transport", default="unspecified")
    parser.add_argument("--codec", default="unknown")
    parser.add_argument("--video-bitrate-mbps", type=float)
    parser.add_argument("--expected-sha256")
    parser.add_argument("--timeout-seconds", type=positive, default=3600)
    args = parser.parse_args()
    if args.expected_sha256 and (len(args.expected_sha256) != 64 or
                                  any(char not in "0123456789abcdefABCDEF" for char in args.expected_sha256)):
        parser.error("--expected-sha256 must contain 64 hexadecimal digits")
    require_windows()
    from vft_windows import ProcessMetrics
    receiver_exe = executable(target_directory(), "vft-recv-win")
    if not receiver_exe.is_file():
        raise RuntimeError("Build release binaries first: cargo build --release --workspace")
    directory = evidence_directory("remote")
    output_path = directory / "recovered.bin"
    stats_path = directory / "stats.csv"
    configuration = dict(
        transport=args.transport, codec=args.codec, video_bitrate_mbps=args.video_bitrate_mbps,
        requested_sender_fps=args.sender_fps, cell_size=args.cell_size, profile=args.profile,
        capture_rect=[args.x, args.y, args.width, args.height], expected_sha256=args.expected_sha256,
        captured_at_utc=datetime.now(timezone.utc).isoformat(),
    )
    with (directory / "configuration.json").open("x", encoding="utf-8") as stream:
        json.dump(configuration, stream, indent=2)
        stream.write("\n")
    with LoggedProcess([
        receiver_exe, "--rect", args.x, args.y, args.width, args.height, "--no-tui",
        "--profile", args.profile, "--output", output_path, "--fps", "120", "--timeout",
        args.timeout_seconds, "--stats", stats_path, "--preview", directory / "preview.bmp",
    ], directory, "receiver") as receiver, ProcessMetrics(receiver) as metrics:
        deadline = time.monotonic() + args.timeout_seconds + 30
        while True:
            try:
                receiver.wait(timeout=1)
                break
            except subprocess.TimeoutExpired:
                metrics.sample()
                if time.monotonic() > deadline:
                    raise RuntimeError(f"Receiver exceeded timeout; diagnostics: {directory}")
        if receiver.returncode != 0:
            raise RuntimeError(f"Receiver failed; diagnostics: {directory}")
        actual = sha256(output_path)
        if args.expected_sha256 and args.expected_sha256.upper() != actual:
            raise RuntimeError("Input/output SHA-256 mismatch")
        stats = read_stats(stats_path)
        stats.update(metrics.values("receiver"))
        stats.update(sha256=actual)
        write_csv(directory / "result.csv", [stats])
        print(f"PASS SHA256={actual} verified_goodput={stats['verified_goodput_bytes_per_second']} B/s")
        print(f"Results: {directory}")


if __name__ == "__main__":
    main(check)
