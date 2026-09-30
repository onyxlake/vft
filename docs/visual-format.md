# Luminance frame v0

Each frame contains an independent packet; decoding does not require arrival order or previous
pixels. The sampler consumes an exact frame rectangle. It can be supplied manually or by the
axis-aligned desktop locator described below. Perspective correction remains unsupported.

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
CRC failures are discarded and recovered with FEC.

## Desktop location

`vft_visual::locate` searches a luminance image for the existing v0 sync bands. It scans every
second image row, finds the regular white runs of the top sync band, and estimates horizontal
origin/width for each standard lattice. The inverse bottom band supplies vertical bounds, so
horizontal and vertical scales can differ. No wire-format change or sender-window metadata is
needed.

Each candidate must pass the full calibration, sync, bootstrap and packet CRC checks. A caller
predicate can reject other session/object identities during reacquisition. The scan examines at
most 128 distinct candidate geometries per image, and image dimensions remain limited to 8192.
Search uses a row-wide black/white threshold and requires at least 75 luminance units of sync
contrast. Severe compression, low contrast, interrupted stripes, partial occlusion or clutter
can prevent discovery; a restricted search area or manual crop is available as a fallback.

`VisualFrame::sample_rect` samples a rectangle within a larger image without copying its pixels.
The Windows `--auto` adapter searches the virtual desktop, including negative monitor origins,
or an optional `--search-rect`. Once found it captures only that region. After 750 ms without a
valid packet from the selected session it resumes discovery, retaining recovered generations.
Unsuccessful discovery scans are counted separately from rejected frame captures. Rotation and
perspective are not supported; the entire frame must be visible with at least two pixels per cell.

Stable borders, large uniform cells and luminance coding are intentional. The payload still changes
between frames, and temporal codec performance needs real RDP/H.264/H.265/AV1 measurements.
Synthetic blur/noise do not establish video codec performance. Two/four-pixel cells can accelerate
local testing but are not recommended as remote compression profiles.
