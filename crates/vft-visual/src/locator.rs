use crate::{GridConfig, ImageRect, LumaImage, VisualFrame};
use alloc::vec::Vec;
use vft_protocol::{Packet, ProfileId};

#[derive(Clone, Debug)]
pub struct LocatedFrame {
    pub rect: ImageRect,
    pub packet: Packet,
}

/// Locate a standard v0 lattice in an axis-aligned desktop image. Candidates require
/// a complete bootstrap and CRC-valid packet. `accept` can restrict discovery to an
/// already selected session/object. Search work is bounded to 128 distinct candidates.
pub fn locate(
    image: &LumaImage,
    profile: Option<ProfileId>,
    mut accept: impl FnMut(&Packet) -> bool,
) -> Result<Option<LocatedFrame>, crate::Error> {
    image.validate()?;
    let mut examined = Vec::new();
    for y in (0..image.height).step_by(2) {
        let row = &image.pixels[y * image.width..(y + 1) * image.width];
        let low = *row.iter().min().unwrap();
        let high = *row.iter().max().unwrap();
        if high.saturating_sub(low) < 75 {
            continue;
        }
        let threshold = (u16::from(low) + u16::from(high)) / 2;
        let mut runs = Vec::new();
        let mut start = None;
        for (x, &value) in row.iter().enumerate() {
            if u16::from(value) > threshold {
                start.get_or_insert(x);
            } else if let Some(begin) = start.take() {
                runs.push((begin, x));
            }
        }
        if let Some(begin) = start {
            runs.push((begin, image.width));
        }
        for candidate in [ProfileId::Balanced, ProfileId::Robust] {
            if profile.is_some_and(|p| p != candidate) {
                continue;
            }
            let grid = GridConfig::for_profile(candidate);
            let count = grid.columns / 16;
            for group in runs.windows(count) {
                // First white stripe starts at cell 8; last ends before the black border.
                let span = group[count - 1].1 - group[0].0;
                let width = (span * grid.columns + (grid.columns - 9) / 2) / (grid.columns - 9);
                if width < grid.columns * 2 || width > image.width {
                    continue;
                }
                let offset = (8 * width + grid.columns / 2) / grid.columns;
                let Some(x) = group[0].0.checked_sub(offset) else {
                    continue;
                };
                if x + width > image.width {
                    continue;
                }
                let tolerance = 2.max(width / grid.columns / 4);
                if group.iter().enumerate().any(|(i, &(begin, end))| {
                    let expected_begin = x + (8 + 16 * i) * width / grid.columns;
                    let expected_end =
                        x + (16 + 16 * i).min(grid.columns - 1) * width / grid.columns;
                    begin.abs_diff(expected_begin) > tolerance
                        || end.abs_diff(expected_end) > tolerance
                }) {
                    continue;
                }
                if !sync_row(image, x, width, y, grid, false) {
                    continue;
                }
                let mut top = y;
                while top > 0 && sync_row(image, x, width, top - 1, grid, false) {
                    top -= 1;
                }
                let key = (x, width, top, candidate);
                if examined.contains(&key) {
                    continue;
                }
                if examined.len() == 128 {
                    return Ok(None);
                }
                examined.push(key);
                let mut top_end = y + 1;
                while top_end < image.height && sync_row(image, x, width, top_end, grid, false) {
                    top_end += 1;
                }
                let band = top_end - top;
                let min_height = grid.rows * 2;
                // The opposite sync band determines height independently of horizontal scale.
                let mut bottom = top + min_height - 2;
                while bottom < image.height {
                    if !sync_row(image, x, width, bottom, grid, true) {
                        bottom += 1;
                        continue;
                    }
                    let mut end = bottom + 1;
                    while end < image.height && sync_row(image, x, width, end, grid, true) {
                        end += 1;
                    }
                    let height = end - top;
                    if height >= min_height
                        && (height / grid.rows).abs_diff(band) <= 2
                        && (end - bottom).abs_diff(band) <= 2
                    {
                        let rect = ImageRect {
                            x,
                            y: top,
                            width,
                            height,
                        };
                        if let Ok(packet) =
                            VisualFrame::sample_rect(image, grid, rect).and_then(|f| f.decode())
                        {
                            if packet.profile == candidate && accept(&packet) {
                                return Ok(Some(LocatedFrame { rect, packet }));
                            }
                        }
                    }
                    bottom = end;
                }
            }
        }
    }
    Ok(None)
}

fn sync_row(
    image: &LumaImage,
    x: usize,
    width: usize,
    y: usize,
    grid: GridConfig,
    inverse: bool,
) -> bool {
    let mut darkest_white = 255u8;
    let mut brightest_black = 0u8;
    for cell in (4..grid.columns - 1).step_by(8) {
        let px = x + (2 * cell + 1) * width / (2 * grid.columns);
        let value = image.pixels[y * image.width + px];
        if ((cell / 8) % 2 != 0) ^ inverse {
            darkest_white = darkest_white.min(value);
        } else {
            brightest_black = brightest_black.max(value);
        }
    }
    darkest_white.saturating_sub(brightest_black) >= 75
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;
    use vft_protocol::{PacketType, SymbolId};

    fn packet(profile: ProfileId) -> Packet {
        Packet {
            session: [1; 16],
            object: [2; 16],
            kind: PacketType::RepairSymbol,
            profile,
            symbol: SymbolId {
                generation: 0,
                index: 64,
            },
            payload: vec![53; 200],
        }
    }
    fn desktop(frame: &LumaImage, x: usize, y: usize) -> LumaImage {
        let mut image = LumaImage {
            width: frame.width + 137,
            height: frame.height + 111,
            pixels: vec![113; (frame.width + 137) * (frame.height + 111)],
        };
        for row in 0..frame.height {
            let start = (row + y) * image.width + x;
            image.pixels[start..start + frame.width]
                .copy_from_slice(&frame.pixels[row * frame.width..(row + 1) * frame.width]);
        }
        image
    }
    #[test]
    fn locates_both_profiles_at_offsets_and_screen_edges() {
        for profile in [ProfileId::Robust, ProfileId::Balanced] {
            for cell in [2, 4, 8] {
                let packet = packet(profile);
                let frame = VisualFrame::encode(&packet, GridConfig::for_profile(profile))
                    .unwrap()
                    .render(cell)
                    .unwrap();
                for (x, y) in [(0, 0), (37, 29)] {
                    let image = desktop(&frame, x, y);
                    let found = locate(&image, None, |_| true).unwrap().unwrap();
                    assert_eq!(
                        found.rect,
                        ImageRect {
                            x,
                            y,
                            width: frame.width,
                            height: frame.height
                        }
                    );
                    assert_eq!(found.packet, packet);
                }
                assert!(locate(&frame, Some(profile), |_| true).unwrap().is_some());
            }
        }
    }
    #[test]
    fn rejects_corruption_false_patterns_wrong_profile_and_session() {
        let packet = packet(ProfileId::Robust);
        let mut frame =
            VisualFrame::encode(&packet, GridConfig::for_profile(packet.profile)).unwrap();
        let image = frame.render(4).unwrap();
        assert!(locate(&image, Some(ProfileId::Balanced), |_| true)
            .unwrap()
            .is_none());
        assert!(locate(&image, None, |p| p.session == [9; 16])
            .unwrap()
            .is_none());
        frame.cells[3 * frame.grid.columns + 1] ^= 255;
        assert!(
            locate(&desktop(&frame.render(4).unwrap(), 37, 29), None, |_| true)
                .unwrap()
                .is_none()
        );
        let noise = LumaImage {
            width: 512,
            height: 256,
            pixels: (0..512 * 256)
                .map(|i| ((i * 37 + i / 512 * 97) % 256) as u8)
                .collect(),
        };
        assert!(locate(&noise, None, |_| true).unwrap().is_none());
        assert!(locate(
            &LumaImage {
                width: 1,
                height: 1,
                pixels: vec![]
            },
            None,
            |_| true
        )
        .is_err());
        assert!(image
            .crop(ImageRect {
                x: usize::MAX,
                y: 0,
                width: 1,
                height: 1
            })
            .is_err());
    }
}
