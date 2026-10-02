use vft_protocol::{
    repair_request::{RepairRequest, Target},
    Digest, Manifest, PacketType, ProfileId, Sha256,
};
use vft_receiver::ReceiverSession;
use vft_sender::{SenderConfig, SenderSession};
use vft_visual::{GridConfig, VisualFrame};

fn run(profile: ProfileId, manual: bool, late: bool) -> (u64, u64) {
    let data: Vec<u8> = (0..10240).map(|i| vft_fec::splitmix64(i) as u8).collect();
    let m = Manifest {
        session: [3; 16],
        object: [4; 16],
        filename: String::from("manual.bin"),
        file_size: data.len() as u64,
        sha256: Sha256::digest(&data).into(),
        symbol_size: 16,
        source_symbols: 640,
        profile,
    };
    let mut sender = SenderSession::new(
        m.clone(),
        &data[..],
        SenderConfig {
            grid: GridConfig::for_profile(profile),
            manifest_interval: 30,
        },
    )
    .unwrap();
    let mut receiver = ReceiverSession::default();
    let mut request_id = 0;
    let mut pending = Vec::new();
    let mut tail_start = 0;
    for seq in 0..15000 {
        if manual
            && receiver.manifest().is_some()
            && sender.progress().source_symbols == 640
            && sender.manual_remaining() == 0
            && !receiver.is_complete()
        {
            let targets = receiver
                .generation_progress()
                .filter(|g| !g.complete)
                .map(|g| Target {
                    generation: g.generation,
                    deficit: Some(g.source_count - g.rank),
                })
                .collect();
            request_id += 1;
            sender
                .apply_request(RepairRequest {
                    id: Some(request_id),
                    targets,
                })
                .unwrap();
        }
        if tail_start == 0 && receiver.progress().completed_generations >= 8 {
            tail_start = seq;
        }
        let p = sender.next_packet().unwrap();
        if late && seq < 900 {
            continue;
        }
        // Same sequence-based random/burst-loss trace in the baseline and manual cases.
        if vft_fec::splitmix64(seq ^ 37) % 100 < 18 || seq % 97 >= 85 {
            continue;
        }
        if !late && p.kind == PacketType::SourceSymbol && [0, 9].contains(&p.symbol.generation) {
            continue;
        }
        let wire = VisualFrame::encode(&p, GridConfig::for_profile(profile))
            .unwrap()
            .decode()
            .unwrap()
            .encode()
            .unwrap();
        pending.push(wire.clone());
        if seq % 7 == 0 {
            pending.push(wire);
        }
        if pending.len() < 8 {
            continue;
        }
        // Reorder and duplicate arrivals; CRC-validated core still owns completion.
        for wire in pending.drain(..).rev() {
            receiver.push_packet(&wire).unwrap();
        }
        if receiver.is_complete() {
            let recovered = receiver.reconstruct().unwrap();
            assert_eq!(recovered, data);
            assert_eq!(Sha256::digest(&recovered), Sha256::digest(&data));
            return (seq + 1, seq.saturating_sub(tail_start));
        }
    }
    panic!(
        "Did not recover: {profile:?} manual={manual} late={late} {:?}",
        receiver.progress()
    );
}
#[test]
fn targeted_early_and_last_groups_reduce_seeded_tail() {
    for profile in [ProfileId::Robust, ProfileId::Balanced] {
        let baseline = run(profile, false, false);
        let manual = run(profile, true, false);
        println!("{profile:?}: baseline frames/tail={baseline:?}, manual frames/tail={manual:?}");
        assert!(manual.0 < baseline.0);
        assert!(manual.1 < baseline.1);
        run(profile, true, true);
    }
}
