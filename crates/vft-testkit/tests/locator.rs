use vft_protocol::{Packet, PacketType, ProfileId, SymbolId};
use vft_testkit::{distort, resize, Distortion};
use vft_visual::{locate, GridConfig, ImageRect, LumaImage, VisualFrame};

fn embed(desktop: &mut LumaImage, frame: &LumaImage, x: usize, y: usize) {
    for row in 0..frame.height {
        let start = (y + row) * desktop.width + x;
        desktop.pixels[start..start + frame.width]
            .copy_from_slice(&frame.pixels[row * frame.width..(row + 1) * frame.width]);
    }
}

#[test]
fn scaled_noisy_frames_move_and_reject_occlusion() {
    for profile in [ProfileId::Robust, ProfileId::Balanced] {
        let packet = Packet {
            session: [3; 16],
            object: [4; 16],
            profile,
            kind: PacketType::SourceSymbol,
            symbol: SymbolId {
                generation: 0,
                index: 0,
            },
            payload: (0..350).map(|i| (i * 31) as u8).collect(),
        };
        let grid = GridConfig::for_profile(profile);
        let rendered = VisualFrame::encode(&packet, grid)
            .unwrap()
            .render(8)
            .unwrap();
        // Independent, fractional horizontal/vertical scaling, then simulated compression effects.
        let scaled = resize(&rendered, 1177, 643);
        let scaled = distort(
            &scaled,
            Distortion {
                brightness: 11,
                contrast_percent: 85,
                blur_radius: 1,
                noise: 3,
                ..Default::default()
            },
            19,
        )
        .unwrap();
        for (x, y) in [(19, 17), (145, 89)] {
            let mut desktop = LumaImage {
                width: 1400,
                height: 760,
                pixels: vec![105; 1400 * 760],
            };
            embed(&mut desktop, &scaled, x, y);
            let found = locate(&desktop, None, |_| true).unwrap().unwrap();
            assert_eq!(found.packet, packet);
            assert!(found.rect.x.abs_diff(x) <= 2 && found.rect.width.abs_diff(scaled.width) <= 2);
            assert_eq!(found.rect.y, y);
            assert_eq!(found.rect.height, scaled.height);
            // A title bar/background is outside the located rectangle; a covered frame is rejected.
            for row in y..y + 100 {
                let start = row * desktop.width + x;
                desktop.pixels[start..start + 250].fill(105);
            }
            assert!(locate(&desktop, None, |_| true).unwrap().is_none());
        }
    }
}

#[test]
fn reacquisition_selects_existing_session_among_multiple_senders() {
    let mut desktop = LumaImage {
        width: 1200,
        height: 700,
        pixels: vec![100; 1200 * 700],
    };
    for (session, x, y) in [(1, 0, 0), (2, 600, 350)] {
        let packet = Packet {
            session: [session; 16],
            object: [2; 16],
            profile: ProfileId::Robust,
            kind: PacketType::RepairSymbol,
            symbol: SymbolId {
                generation: 0,
                index: 64,
            },
            payload: vec![23; 200],
        };
        let frame = VisualFrame::encode(&packet, GridConfig::for_profile(packet.profile))
            .unwrap()
            .render(4)
            .unwrap();
        embed(&mut desktop, &frame, x, y);
    }
    let found = locate(&desktop, None, |p| p.session == [2; 16])
        .unwrap()
        .unwrap();
    assert_eq!(
        found.rect,
        ImageRect {
            x: 600,
            y: 350,
            width: 384,
            height: 216
        }
    );
    assert_eq!(found.packet.session, [2; 16]);
}
