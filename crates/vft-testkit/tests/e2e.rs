use vft_protocol::{Digest, Manifest, ProfileId, Sha256};
use vft_receiver::ReceiverSession;
use vft_sender::{SenderConfig, SenderSession};
use vft_testkit::{Distortion, Transport};
use vft_visual::{GridConfig, VisualFrame};

fn transfer(size: usize, profile: ProfileId, late_join: bool, distorted: bool) {
    let data: Vec<u8> = (0..size)
        .map(|i| vft_fec::splitmix64(i as u64) as u8)
        .collect();
    let grid = if size >= 1024 * 1024 {
        GridConfig::for_profile(profile)
    } else {
        GridConfig {
            columns: 96,
            rows: 24,
        }
    };
    let symbol_size = grid.payload_capacity(profile).unwrap() as u32;
    let manifest = Manifest {
        session: [8; 16],
        object: [7; 16],
        filename: "fixture.bin".into(),
        file_size: size as u64,
        sha256: Sha256::digest(&data).into(),
        symbol_size,
        source_symbols: (size as u64).div_ceil(u64::from(symbol_size)) as u32,
        profile,
    };
    let mut sender = SenderSession::new(
        manifest.clone(),
        &data[..],
        SenderConfig {
            grid,
            manifest_interval: 16,
        },
    )
    .unwrap();
    let mut receiver = ReceiverSession::default();
    let transport = Transport {
        seed: 15,
        drop_percent: 20,
        duplicate_percent: 35,
        cell_size: if distorted { 8 } else { 2 },
        distortion: if distorted {
            Distortion {
                scale_percent: 75,
                blur_radius: 1,
                brightness: 12,
                contrast_percent: 85,
                noise: 4,
                ..Default::default()
            }
        } else {
            Distortion::default()
        },
    };
    let start = if late_join {
        u64::from(manifest.source_symbols) * 2 + 40
    } else {
        0
    };
    let budget = start + u64::from(manifest.source_symbols) * 10 + 512;
    let mut invalid_frames = 0;
    let mut batch = Vec::new();
    for sequence in 0..budget {
        let frame = sender.next_frame().unwrap();
        if sequence < start {
            continue;
        }
        // Force whole-frame corruption in addition to stochastic loss and distortion.
        let mut frame = frame;
        if sequence % 11 == 3 {
            frame.cells[3 * grid.columns + 1] ^= 255;
        }
        batch.extend(transport.transmit(&frame, sequence).unwrap());
        if batch.len() < 7 {
            continue;
        }
        transport.reorder(&mut batch);
        for image in batch.drain(..) {
            match VisualFrame::sample(&image, grid).and_then(|f| f.decode()) {
                Ok(packet) => {
                    receiver.push_packet(&packet.encode().unwrap()).unwrap();
                }
                Err(_) => {
                    invalid_frames += 1;
                }
            }
        }
        if receiver.is_complete() {
            break;
        }
    }
    assert!(receiver.is_complete(),"size={size}, profile={profile:?}, late={late_join}, invalid={invalid_frames}, progress={:?}",receiver.progress());
    let output = receiver.reconstruct().unwrap();
    assert_eq!(output, data);
    assert_eq!(Sha256::digest(output), Sha256::digest(data));
    if size > 1024 {
        assert!(receiver.progress().duplicate_symbols > 0);
        assert!(invalid_frames > 0);
    }
}

#[test]
fn files_through_loss_duplicates_distortion_and_reordering() {
    for profile in [ProfileId::Robust, ProfileId::Balanced] {
        for size in [0, 1, 1024, 33001] {
            transfer(size, profile, false, true);
        }
    }
}
#[test]
fn receiver_joins_after_all_sources_are_gone() {
    for profile in [ProfileId::Robust, ProfileId::Balanced] {
        transfer(33001, profile, true, true);
    }
}
#[test]
fn one_megabyte_visual_transfer() {
    transfer(1024 * 1024, ProfileId::Balanced, false, false);
}

#[test]
fn streaming_source_memory_does_not_grow_with_file_size() {
    struct Generated {
        max_read: usize,
        reads: u64,
    }
    impl vft_sender::ObjectSource for Generated {
        fn read_exact_at(&mut self, offset: u64, buffer: &mut [u8]) -> Result<(), String> {
            self.max_read = self.max_read.max(buffer.len());
            self.reads += 1;
            for (i, b) in buffer.iter_mut().enumerate() {
                *b = (offset + i as u64) as u8;
            }
            assert!(buffer.len() <= 4096);
            Ok(())
        }
    }
    let size = 100 * 1024 * 1024;
    let manifest = Manifest {
        session: [0; 16],
        object: [1; 16],
        filename: "large.bin".into(),
        file_size: size,
        sha256: [0; 32],
        symbol_size: 4096,
        source_symbols: (size / 4096) as u32,
        profile: ProfileId::Balanced,
    };
    let mut sender = SenderSession::new(
        manifest,
        Generated {
            max_read: 0,
            reads: 0,
        },
        SenderConfig {
            grid: GridConfig::for_profile(ProfileId::Balanced),
            manifest_interval: 32,
        },
    )
    .unwrap();
    // Cross several generation boundaries; the source generates bytes on demand.
    for _ in 0..200 {
        sender.next_packet().unwrap();
    }
    assert!(sender.progress().source_symbols > 128);
}
