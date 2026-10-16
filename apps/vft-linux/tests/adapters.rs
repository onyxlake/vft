use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};
use vft_linux::{open_sender, packed_luma, save_preview, save_verified, Decoder, PixelOrder};
use vft_protocol::{Digest, ProfileId, Sha256};
use vft_visual::{ImageRect, LumaImage};
static NEXT: AtomicU64 = AtomicU64::new(0);
fn fixture(bytes: &[u8]) -> PathBuf {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../output/linux-adapter-tests");
    fs::create_dir_all(&root).unwrap();
    let dir = root.join(format!(
        "{}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&dir).unwrap();
    let path = dir.join("input.bin");
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .unwrap()
        .write_all(bytes)
        .unwrap();
    path
}
fn place(image: &LumaImage, x: usize, y: usize) -> LumaImage {
    let mut out = LumaImage {
        width: image.width + 40,
        height: image.height + 40,
        pixels: vec![37; (image.width + 40) * (image.height + 40)],
    };
    for row in 0..image.height {
        let start = (row + y) * out.width + x;
        out.pixels[start..start + image.width]
            .copy_from_slice(&image.pixels[row * image.width..(row + 1) * image.width]);
    }
    out
}
#[test]
fn packed_formats_padding_negative_stride_and_limits() {
    let bgra = [
        0, 0, 255, 255, 0, 255, 0, 0, 99, 99, 99, 99, 255, 0, 0, 0, 255, 255, 255, 0,
    ];
    assert_eq!(
        packed_luma(&bgra, 2, 2, 12, PixelOrder::Bgr)
            .unwrap()
            .pixels,
        [76, 149, 28, 255]
    );
    assert_eq!(
        packed_luma(&bgra, 2, 2, -12, PixelOrder::Bgr)
            .unwrap()
            .pixels,
        [28, 255, 76, 149]
    );
    assert_eq!(
        packed_luma(&bgra, 2, 2, 12, PixelOrder::Rgb)
            .unwrap()
            .pixels,
        [28, 149, 76, 255]
    );
    for (w, h, stride) in [
        (0, 1, 4),
        (8193, 1, 32772),
        (1, 8193, 4),
        (2, 2, 7),
        (2, 2, 20),
        (1, 2, i32::MIN),
    ] {
        assert!(packed_luma(&bgra, w, h, stride, PixelOrder::Rgb).is_err());
    }
    assert!(packed_luma(&bgra[..19], 2, 2, 12, PixelOrder::Bgr).is_err());
}
#[test]
fn file_source_detects_size_change() {
    let path = fixture(&[7; 100]);
    let mut sender = open_sender(&path, ProfileId::Robust).unwrap();
    OpenOptions::new()
        .append(true)
        .open(path)
        .unwrap()
        .write_all(&[8])
        .unwrap();
    // First manifest needs no source read; next source must fail.
    sender.next_frame().unwrap();
    assert!(sender.next_frame().is_err());
}
#[test]
fn verified_recovery_from_padded_rgb_capture_with_loss_reorder_duplicates_and_late_join() {
    for profile in [ProfileId::Robust, ProfileId::Balanced] {
        let size = if profile == ProfileId::Balanced {
            350001
        } else {
            40001
        };
        let bytes: Vec<u8> = (0..size).map(|n| ((n * 73 + n / 17) % 256) as u8).collect();
        let path = fixture(&bytes);
        let mut sender = open_sender(&path, profile).unwrap();
        // Join after all systematic symbols. Recover from repairs and repeated manifests only.
        while sender.progress().source_symbols < u64::from(sender.manifest().source_symbols) {
            sender.next_frame().unwrap();
        }
        let mut decoder = Decoder::new(None, None, None, 1024 * 1024);
        let mut now = Instant::now();
        for batch in 0..100 {
            let mut captures = Vec::new();
            for i in 0..7 {
                let frame = sender.next_frame().unwrap();
                if (i + batch) % 5 == 0 {
                    continue;
                }
                let image = place(&frame.render(2).unwrap(), 12, 16);
                let stride = image.width * 4 + 12;
                let mut raw = vec![99; stride * image.height + 8];
                for y in 0..image.height {
                    for x in 0..image.width {
                        let v = image.pixels[y * image.width + x];
                        let start = 8 + y * stride + x * 4;
                        raw[start..start + 4].copy_from_slice(&[v, v, v, 255]);
                    }
                }
                let image = packed_luma(
                    &raw[8..],
                    image.width,
                    image.height,
                    stride as i32,
                    PixelOrder::Bgr,
                )
                .unwrap();
                captures.push(image);
            }
            captures.reverse();
            for image in captures {
                now += Duration::from_millis(501);
                decoder.push(&image, now).unwrap();
                decoder.push(&image, now).unwrap();
                if decoder.receiver.is_complete() {
                    break;
                }
            }
            if decoder.receiver.is_complete() {
                break;
            }
        }
        assert!(decoder.receiver.is_complete(), "{profile:?}");
        assert_eq!(decoder.receiver.reconstruct().unwrap(), bytes);
        let output = path.with_file_name("recovered.bin");
        save_verified(&decoder.receiver, &output).unwrap();
        let actual = fs::read(&output).unwrap();
        assert_eq!(Sha256::digest(&actual), Sha256::digest(&bytes));
        assert!(save_verified(&decoder.receiver, &output).is_err());
        assert_eq!(fs::read(&output).unwrap(), bytes);
    }
}
#[test]
fn movement_resize_and_foreign_session_do_not_discard_progress() {
    let path = fixture(&[9; 5000]);
    let mut sender = open_sender(&path, ProfileId::Balanced).unwrap();
    let first = sender.next_frame().unwrap().render(2).unwrap();
    let mut decoder = Decoder::new(None, None, None, 10000);
    let now = Instant::now();
    let mut limited = Decoder::new(None, None, None, 4999);
    assert!(limited.push(&place(&first, 2, 2), now).is_err());
    assert!(!limited.receiver.is_complete());
    decoder.push(&place(&first, 2, 2), now).unwrap();
    let identity = decoder.receiver.manifest().unwrap().session;
    let other_path = fixture(&[8; 5000]);
    let mut other = open_sender(&other_path, ProfileId::Balanced).unwrap();
    let other = other.next_frame().unwrap().render(2).unwrap();
    decoder
        .push(&place(&other, 2, 2), now + Duration::from_secs(1))
        .unwrap();
    decoder
        .push(&place(&other, 2, 2), now + Duration::from_secs(2))
        .unwrap();
    assert!(decoder.lock.is_none());
    decoder
        .push(&place(&other, 2, 2), now + Duration::from_secs(3))
        .unwrap();
    assert!(decoder.lock.is_none());
    decoder
        .push(&place(&first, 20, 20), now + Duration::from_secs(4))
        .unwrap();
    assert_eq!(decoder.locks, 2);
    assert_eq!(decoder.receiver.manifest().unwrap().session, identity);
    // Stream-size change invalidates location immediately.
    decoder.push(&first, now + Duration::from_secs(5)).unwrap();
    assert_eq!(decoder.lock.unwrap().x, 0);
    assert_eq!(decoder.locks, 3);
}
#[test]
fn manual_rect_search_rect_empty_and_create_new_diagnostics() {
    let path = fixture(&[]);
    let mut sender = open_sender(&path, ProfileId::Robust).unwrap();
    let image = sender.next_frame().unwrap().render(2).unwrap();
    let rect = ImageRect {
        x: 0,
        y: 0,
        width: image.width,
        height: image.height,
    };
    let output = path.with_file_name("empty.bin");
    let mut decoder = Decoder::new(None, Some(rect), None, 0);
    assert!(save_verified(&decoder.receiver, &output).is_err());
    assert!(!output.exists());
    decoder.push(&image, Instant::now()).unwrap();
    assert!(decoder.receiver.is_complete());
    save_verified(&decoder.receiver, &output).unwrap();
    assert_eq!(fs::metadata(output).unwrap().len(), 0);
    let preview = path.with_file_name("preview.bmp");
    save_preview(&image, &preview).unwrap();
    assert!(save_preview(&image, &preview).is_err());
    assert_eq!(&fs::read(preview).unwrap()[..2], b"BM");
    let mut decoder = Decoder::new(
        None,
        None,
        Some(ImageRect {
            x: 12,
            y: 16,
            ..rect
        }),
        0,
    );
    decoder
        .push(&place(&image, 12, 16), Instant::now())
        .unwrap();
    assert_eq!(decoder.lock.unwrap().x, 12);
    let mut decoder = Decoder::new(
        None,
        Some(ImageRect {
            x: usize::MAX,
            ..rect
        }),
        None,
        0,
    );
    assert!(decoder.push(&image, Instant::now()).is_err());
}
