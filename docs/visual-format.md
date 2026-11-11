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

Coordinates are zero-based over the complete `C`-column, `R`-row grid, including the border.
Cells are stored row-major at `y * C + x`. All cells start black. Columns `x=0` and `x=C-1`
stay black on every row, including the sync rows. For interior columns `1 <= x <= C-2`:

```text
top(x)        = 255 * (floor(x / 8) mod 2)       at y = 0
bottom(x)     = 255 - top(x)                    at y = R-1
calibration(x)= 85 * floor(4 * (x-1) / (C-2))   at y = 1
```

The sync phase uses the full-grid `x`, not `x-1`: the first interior black run has seven
cells (`x=1..7`), followed by eight white cells (`x=8..15`). The last white run is clipped
by the black right border. Only the interior bottom row is the inverse of the top row.

The four calibration levels are 0, 85, 170 and 255. Their integer widths are:

| Grid columns | Patch widths from left to right | Sample columns for levels 0..3 |
| --- | --- | --- |
| 96 | 24, 23, 24, 23 | 12, 36, 59, 83 |
| 192 | 48, 47, 48, 47 | 24, 72, 119, 167 |

Calibration sample column `j` is `1 + floor((C-2) * (2*j+1) / 8)`, for `j=0..3`.
Row `y=2`, columns `x=1..80`, contains an 80-cell binary bootstrap:

```text
ASCII VFG0 [4 bytes] | profile [1] | reserved=0 [1] | packet_byte_length_le32 [4]
```

The bootstrap length is the complete encoded packet length, including its header and CRC.
The length is little endian; bits within each bootstrap byte are rendered most significant
first, with 0 mapped to black and 1 to white. Remaining interior cells in row 2 stay black.

Packet data starts at `(x=1, y=3)`. Data cell number `n` maps to
`x = 1 + (n mod (C-2))`, `y = 3 + floor(n / (C-2))`, ending before the bottom sync row.
Bits within each byte are most significant first. Robust uses eight cells per byte (0 → 0,
1 → 255); balanced uses four cells (00 → 0, 01 → 85, 10 → 170, 11 → 255).
Unused data cells, including any incomplete trailing byte capacity, remain black.

For example, balanced bytes `1B E4` map to luminance cells
`0, 85, 170, 255, 255, 170, 85, 0`. Robust byte `81` maps to
`255, 0, 0, 0, 0, 0, 0, 255`. Packet capacity is
`floor((C-2) * (R-4) * bits_per_cell / 8)` bytes.

The bootstrap uses one bit per cell regardless of profile. Lattice dimensions are part of the
selected profile; a custom logical lattice must be provided by both callers. The Windows receiver
tries the two standard lattices on every frame until a manifest selects the session profile.

## Sampling and rejection

Sampling averages nine positions per cell at 3/8, 4/8 and 5/8 of its dimensions. For a capture
rectangle `(X, Y, W, H)`, sample cell `(x, y)` at
`(X + floor((8*x+dx)*W/(8*C)), Y + floor((8*y+dy)*H/(8*R)))` for every `dx,dy` in `{3,4,5}`.
The sampled luminance is `floor(sum / 9)`. The capture must have at least two pixels per cell.
Rotated frames, wrong crops, occlusion and strong compression can cause rejection.

Calibration levels must increase by at least 25 luminance units between adjacent patches.
Binary decoding uses `threshold = floor((level[0] + level[3]) / 2)`; values strictly above it
decode to 1, others to 0. Balanced selects the nearest calibrated level, choosing the lower index
on a tie. Sync mismatches across the `2*(C-2)` interior sync cells must be at most 5%.
Bootstrap magic/reserved/profile/length must validate, then the packet CRC must pass. The packet
profile must match the bootstrap. CRC failures are discarded and recovered with FEC.

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
