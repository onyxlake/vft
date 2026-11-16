mod common;
use common::*;
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::Path,
    process::Command,
};
use vft_git::{Control, ReceiveSession, DEFAULT_MAX_PACKAGE};
use vft_protocol::{Manifest, PacketType, ProfileId};
use vft_receiver::{Limits, ReceiverSession};
use vft_sender::{ObjectSource, SenderConfig, SenderSession};
use vft_visual::{GridConfig, VisualFrame};
struct Source(File);
impl ObjectSource for Source {
    fn read_exact_at(&mut self, offset: u64, buffer: &mut [u8]) -> Result<(), String> {
        self.0
            .seek(SeekFrom::Start(offset))
            .and_then(|_| self.0.read_exact(buffer))
            .map_err(|e| e.to_string())
    }
}
fn independent_sha(path: &Path) -> [u8; 32] {
    let out=Command::new(if cfg!(windows) {"python"} else {"python3"})
        .args(["-c", "import hashlib,sys; h=hashlib.sha256(); f=open(sys.argv[1],'rb'); [h.update(b) for b in iter(lambda:f.read(65536), b'')]; sys.stdout.buffer.write(h.digest())"])
        .arg(path).output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    out.stdout.try_into().unwrap()
}
#[test]
fn git_package_visual_frames_repair_only_recovery_and_automatic_application() {
    for profile in [ProfileId::Robust, ProfileId::Balanced] {
        let (root, source, receiver, base) = fixture();
        advance(&source);
        let binary: Vec<_> = (0..16384u32)
            .flat_map(|n| n.wrapping_mul(2654435761).to_le_bytes())
            .collect();
        fs::write(source.join("data.bin"), binary).unwrap();
        fs::write(source.join(".gitattributes"), b"data.bin -text\n").unwrap();
        git(&source, &["add", "."]);
        git(&source, &["commit", "-qm", "binary fixture"]);
        let target = git(&source, &["rev-parse", "HEAD"]);
        let package = package(&source, &base, &root.join("input.vftgit"));
        let expected = independent_sha(&package);
        let len = fs::metadata(&package).unwrap().len();
        let grid = GridConfig::for_profile(profile);
        let symbol_size = grid.payload_capacity(profile).unwrap() as u32;
        let manifest = Manifest {
            session: [17; 16],
            object: expected[..16].try_into().unwrap(),
            filename: "input.vftgit".into(),
            file_size: len,
            sha256: expected,
            symbol_size,
            source_symbols: len.div_ceil(symbol_size as u64) as u32,
            profile,
        };
        let mut sender = SenderSession::new(
            manifest,
            Source(File::open(&package).unwrap()),
            SenderConfig {
                grid,
                manifest_interval: 15,
            },
        )
        .unwrap();
        let mut recovery = ReceiverSession::new(Limits::default());
        let mut batch = Vec::new();
        for n in 0..6000 {
            let frame = sender.next_frame().unwrap();
            // Late joining, all sources lost, additional repair loss and reversed batches.
            if n < 10 || n % 7 == 0 || frame.decode().unwrap().kind == PacketType::SourceSymbol {
                continue;
            }
            batch.push(frame);
            if batch.len() < 4 {
                continue;
            }
            for frame in batch.drain(..).rev() {
                let image = frame.render(2).unwrap();
                let sampled = VisualFrame::sample(&image, grid).unwrap();
                let packet = sampled.decode().unwrap().encode().unwrap();
                let mut corrupt = packet.clone();
                let last = corrupt.len() - 1;
                corrupt[last] ^= 1;
                assert!(recovery.push_packet(&corrupt).is_err());
                recovery.push_packet(&packet).unwrap();
                if !recovery.is_complete() {
                    recovery.push_packet(&packet).unwrap();
                }
                if recovery.is_complete() {
                    break;
                }
            }
            if recovery.is_complete() {
                break;
            }
        }
        assert!(recovery.is_complete(), "repair-only recovery: {profile:?}");
        let saved = root.join("received.vftgit");
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&saved)
            .unwrap();
        for chunk in recovery.verified_chunks().unwrap() {
            file.write_all(chunk).unwrap();
        }
        file.sync_all().unwrap();
        drop(file);
        assert_eq!(independent_sha(&saved), expected);
        assert_eq!(fs::read(&saved).unwrap(), fs::read(&package).unwrap());
        let session = ReceiveSession::open(&receiver, None, &Control::default()).unwrap();
        session
            .apply(&saved, DEFAULT_MAX_PACKAGE, &Control::default(), |_| {})
            .unwrap();
        assert_eq!(git(&receiver, &["rev-parse", "HEAD"]), target);
        assert_eq!(
            git(&receiver, &["rev-parse", "HEAD^{tree}"]),
            git(&source, &["rev-parse", "HEAD^{tree}"])
        );
        assert_eq!(
            independent_sha(&receiver.join("data.bin")),
            independent_sha(&source.join("data.bin"))
        );
    }
}
