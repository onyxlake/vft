# Luminance frame v0

Each frame contains an independent packet; decoding does not require arrival order or previous
pixels. The capture source supplies the exact frame rectangle. Arbitrary screen location and
perspective correction are outside this phase.

Default lattices:

| Profile | Grid | Default cell | Bits/cell | Packet capacity | Payload capacity |
| --- | --- | --- | --- | --- | --- |
| Robust (0) | 96 × 54 | 16 px | 1 | 587 bytes | 531 bytes |
| Balanced (1) | 192 × 108 | 8 px | 2 | 4,940 bytes | 4,884 bytes |

Both defaults render at 1536 × 864. FPS defaults to 15; select 5–10 for robust remote transports.
Cells use grayscale luminance: robust 0/255; balanced 0/85/170/255. RGB channels are equal;
receiving converts BGRA to integer luminance `(29*B + 150*G + 77*R) >> 8`.

## Layout

Left/right edges are one black cell. Row 0 is a stable sync pattern with eight-cell runs of
black/white; the bottom row is its inverse. Inner row 1 contains four equal calibration patches
(0, 85, 170, 255), sampled at each patch center. Inner row 2 has an 80-cell binary bootstrap:

```text
ASCII VFG0 [4 bytes] | profile [1] | reserved=0 [1] | packet_byte_length_le32 [4]
```

Remaining inner cells in row 2 are black. Packet data begins at inner row 3, proceeds left to
right, then top to bottom, and ends before the bottom sync row. Bits within each byte are most
significant first. Binary uses eight cells/byte, balanced four cells/byte. Unused data cells are black.

The bootstrap uses one bit per cell regardless of profile. Lattice dimensions are part of the
selected profile; a custom logical lattice must be provided by both callers. The Windows receiver
tries the two standard lattices on every frame until a manifest selects the session profile.

## Sampling and rejection

Sampling averages nine positions per cell at 3/8, 4/8 and 5/8 of the cell dimensions. This tolerates
axis-aligned scaling without sampling transition edges. The capture must have at least two pixels
per cell. Rotated frames, wrong crops, occlusion and strong compression can cause rejection.

Calibration levels must be increasing by at least 25 luminance units. Binary uses the midpoint of
the darkest/brightest patches; balanced selects the closest calibrated level. Sync mismatches must
be at most 5%. Bootstrap magic/reserved/profile/length must validate, then the packet CRC must pass.
CRC failures are discarded and recovered with FEC. Sync is a validation check inside a known
rectangle, not a full-screen locator.

Stable borders, large uniform cells and luminance coding are intentional. The payload still changes
between frames, and temporal codec performance needs real RDP/H.264/H.265/AV1 measurements.
Synthetic blur/noise do not establish video codec performance. Two/four-pixel cells can accelerate
local testing but are not recommended as remote compression profiles.
