//! Browser adapter: bounded hashing and the existing simplex sender, without new wire semantics.
use vft_protocol::{Digest, Manifest, ProfileId, SessionId, Sha256};
use vft_sender::{ObjectSource, SenderConfig, SenderSession};
use vft_visual::GridConfig;

pub const HASH_CHUNK: usize = 65_536;

/// Validate before touching the file; scan it in bounded chunks, then retain only the source.
pub fn prepare<S: ObjectSource>(
    filename: String,
    file_size: u64,
    profile: ProfileId,
    session: SessionId,
    mut source: S,
    mut hash_progress: impl FnMut(u64, u64),
) -> Result<SenderSession<S>, String> {
    let grid = GridConfig::for_profile(profile);
    let symbol_size = grid.payload_capacity(profile).map_err(|e| e.to_string())? as u32;
    let source_symbols = u32::try_from(file_size.div_ceil(u64::from(symbol_size)))
        .map_err(|_| String::from("file exceeds protocol symbol limit"))?;
    let mut manifest = Manifest {
        session,
        object: [0; 16],
        filename,
        file_size,
        sha256: [0; 32],
        symbol_size,
        source_symbols,
        profile,
    };
    manifest.validate().map_err(|e| e.to_string())?;
    if manifest.encode().map_err(|e| e.to_string())?.len() > symbol_size as usize {
        return Err(String::from("filename does not fit the manifest frame"));
    }
    let mut hash = Sha256::new();
    let mut buffer = vec![0; HASH_CHUNK];
    let mut offset = 0;
    hash_progress(0, file_size);
    while offset < file_size {
        let length = (file_size - offset).min(HASH_CHUNK as u64) as usize;
        source.read_exact_at(offset, &mut buffer[..length])?;
        hash.update(&buffer[..length]);
        offset += length as u64;
        hash_progress(offset, file_size);
    }
    // Release the hash buffer before the sender allocates its first generation.
    drop(buffer);
    manifest.sha256 = hash.finalize().into();
    manifest.object.copy_from_slice(&manifest.sha256[..16]);
    SenderSession::new(
        manifest,
        source,
        SenderConfig {
            grid,
            manifest_interval: 30,
        },
    )
    .map_err(|e| e.to_string())
}

#[cfg(target_arch = "wasm32")]
mod wasm;

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::RefCell, rc::Rc};
    use vft_receiver::ReceiverSession;

    struct RecordingSource {
        data: Vec<u8>,
        reads: Rc<RefCell<Vec<(u64, usize)>>>,
    }
    impl ObjectSource for RecordingSource {
        fn read_exact_at(&mut self, offset: u64, buffer: &mut [u8]) -> Result<(), String> {
            self.reads.borrow_mut().push((offset, buffer.len()));
            self.data.as_slice().read_exact_at(offset, buffer)
        }
    }

    #[test]
    fn bounded_scan_random_reads_and_both_profiles_recover() {
        let data: Vec<u8> = (0..150_007).map(|i| (i * 17) as u8).collect();
        for profile in [ProfileId::Robust, ProfileId::Balanced] {
            let reads = Rc::new(RefCell::new(Vec::new()));
            let mut progress = Vec::new();
            let mut sender = prepare(
                String::from("测试.bin"),
                data.len() as u64,
                profile,
                [7; 16],
                RecordingSource {
                    data: data.clone(),
                    reads: reads.clone(),
                },
                |done, total| progress.push((done, total)),
            )
            .unwrap();
            assert_eq!(
                progress.last(),
                Some(&(data.len() as u64, data.len() as u64))
            );
            assert_eq!(
                sender.manifest().sha256,
                <[u8; 32]>::from(Sha256::digest(&data))
            );
            assert_eq!(
                *reads.borrow(),
                [(0, 65536), (65536, 65536), (131072, 18935)]
            );
            let mut receiver = ReceiverSession::default();
            for _ in 0..2000 {
                let packet = sender.next_frame().unwrap().decode().unwrap();
                receiver.push_packet(&packet.encode().unwrap()).unwrap();
                if receiver.is_complete() {
                    break;
                }
            }
            assert_eq!(receiver.reconstruct().unwrap(), data);
            assert!(reads
                .borrow()
                .iter()
                .all(|(_, length)| *length <= HASH_CHUNK));
            // Encoding starts with random-access reads back at the first generation.
            assert_eq!(reads.borrow()[3].0, 0);
        }
    }

    #[test]
    fn invalid_metadata_and_unreadable_file_do_not_create_a_session() {
        assert!(prepare(
            String::from("../bad"),
            1,
            ProfileId::Robust,
            [0; 16],
            &b"x"[..],
            |_, _| {}
        )
        .is_err());
        assert!(prepare(
            String::from("x.bin"),
            2,
            ProfileId::Balanced,
            [0; 16],
            &b"x"[..],
            |_, _| {}
        )
        .is_err());
        let sender = prepare(
            String::from("empty.bin"),
            0,
            ProfileId::Robust,
            [0; 16],
            &b""[..],
            |_, _| {},
        )
        .unwrap();
        assert_eq!(
            sender.manifest().sha256,
            <[u8; 32]>::from(Sha256::digest([]))
        );
    }
}
