# Remote desktop: user-reported automatic capture pass

Reported by the user on 2026-09-30 after testing over remote desktop with `--auto`.
The exact test time, remote desktop product/codec, endpoint configuration, profile, cell size,
sender/receiver FPS, resolution and scaling were not supplied. Window movement, reacquisition
and repair-only late joining were not reported for this remote run.

## Evidence

The user supplied this receiver completion line:

```text
COMPLETE sha256=verified bytes=617787 elapsed=22.077s goodput=27.3 KiB/s progress=Progress { received_packets: 654, valid_symbols: 128, duplicate_symbols: 436, dependent_symbols: 1, invalid_packets: 0, ignored_packets: 69, rank_symbols: 127, recovered_bytes: 617787, completed_generations: 2, hash: Verified } invalid_frames=1
```

| Metric | Reported value |
| --- | --- |
| Capture mode | `--auto` |
| Transport | Remote desktop; product and codec unspecified |
| Recovered original bytes | 617,787 |
| Receiver elapsed time | 22.077 s |
| Verified payload goodput | 27.3 KiB/s |
| SHA-256 status | Verified by receiver |
| Received packets | 654 |
| Valid FEC symbols | 128 |
| Duplicate symbols | 436 |
| Dependent equations | 1 |
| Independent/rank symbols | 127 |
| Invalid packets at receiver core | 0 |
| Ignored packets | 69 |
| Completed generations | 2 |
| Invalid visual frame captures | 1 |

## Interpretation

This is direct user-provided evidence of successful automatic screen discovery and file recovery
through a real remote desktop display, including the receiver's full-file SHA-256 verification.
The remote session was not independently inspected. No source/output hash strings,
statistics CSV, exact command lines or independent file-hash comparison were provided.

27.3 KiB/s is the CLI's rounded verified payload goodput, including receiver discovery, decoding,
recovery, verification and output. Dividing the supplied bytes by the rounded elapsed time gives
approximately 27,983 B/s (28.0 decimal KB/s). This run is below the project's 40–100 KB/s remote
target; it establishes functionality, not that throughput target or the complete FPS/profile matrix.

Duplicate symbol count is not a video frame-loss measurement. Capture count, unique-frame rate,
CRC-failure count, FEC overhead, CPU, memory and video bitrate remain unreported; do not infer
them from the supplied progress fields. Profile and FPS must also remain unspecified.
