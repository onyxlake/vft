//! Native counterpart of scripts/web-e2e.mjs: actual WASM cells -> pixels -> receiver -> SHA.
use std::{env, fs::File, io::Read};
use vft_protocol::{Digest, Sha256};
use vft_receiver::ReceiverSession;
use vft_visual::{GridConfig, VisualFrame};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = env::args().collect();
    if args.len() != 3 {
        return Err("usage: verify-frames FRAMES INPUT".into());
    }
    let input = std::fs::read(&args[2])?;
    let mut frames = File::open(&args[1])?;
    let mut header = [0; 16];
    frames.read_exact(&mut header)?;
    if &header[..4] != b"VWT0" {
        return Err("invalid test fixture magic".into());
    }
    let grid = GridConfig {
        columns: u32::from_le_bytes(header[4..8].try_into()?) as usize,
        rows: u32::from_le_bytes(header[8..12].try_into()?) as usize,
    };
    grid.validate()?;
    let count = u32::from_le_bytes(header[12..16].try_into()?);
    let mut receiver = ReceiverSession::default();
    let mut cells = vec![0; grid.columns * grid.rows];
    for _ in 0..count {
        frames.read_exact(&mut cells)?;
        let image = VisualFrame {
            grid,
            cells: cells.clone(),
        }
        .render(4)?;
        let packet = VisualFrame::sample(&image, grid)?.decode()?;
        receiver.push_packet(&packet.encode()?)?;
        if receiver.is_complete() {
            let recovered = receiver.reconstruct()?;
            if recovered != input || Sha256::digest(&recovered) != Sha256::digest(&input) {
                return Err("independent input comparison failed".into());
            }
            println!(
                "COMPLETE sha256=verified bytes={} packets={}",
                input.len(),
                receiver.progress().received_packets
            );
            return Ok(());
        }
    }
    Err("WASM frame fixture did not complete recovery".into())
}
