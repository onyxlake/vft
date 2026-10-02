//! Small scalar WASM ABI. All pointers refer to Rust-owned buffers; JS never allocates/frees them.
use super::prepare;
use std::cell::RefCell;
use vft_protocol::ProfileId;
use vft_sender::{ObjectSource, SenderSession};

#[link(wasm_import_module = "vft")]
extern "C" {
    fn read_at(offset: f64, destination: *mut u8, length: u32) -> i32;
    fn hash_progress(done: f64, total: f64);
}

struct BrowserSource;
impl ObjectSource for BrowserSource {
    fn read_exact_at(&mut self, offset: u64, buffer: &mut [u8]) -> Result<(), String> {
        // The worker import writes exactly this exclusive, live slice, without reentering WASM.
        // prepare() limits file sizes to the protocol range, well below JS's exact integer limit.
        let ok = unsafe { read_at(offset as f64, buffer.as_mut_ptr(), buffer.len() as u32) };
        if ok == 1 {
            Ok(())
        } else {
            Err(String::from("file read failed; keep the input unchanged"))
        }
    }
}

struct State {
    input: [u8; 271], // 16-byte session followed by up to 255 UTF-8 filename bytes.
    sender: Option<SenderSession<BrowserSource>>,
    frame: Vec<u8>,
    error: String,
    digest: [u8; 32],
}
thread_local! {
    static STATE: RefCell<State> = const { RefCell::new(State {
        input: [0; 271], sender: None, frame: Vec::new(), error: String::new(), digest: [0; 32],
    }) };
}

#[no_mangle]
pub extern "C" fn vft_input_ptr() -> *mut u8 {
    STATE.with(|s| s.borrow_mut().input.as_mut_ptr())
}

#[no_mangle]
pub extern "C" fn vft_init(profile: u32, file_size: f64, filename_length: u32) -> i32 {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        state.sender = None;
        state.frame.clear();
        state.error.clear();
        state.digest.fill(0);
        let result = (|| {
            if !file_size.is_finite()
                || !(0.0..=9_007_199_254_740_991.0).contains(&file_size)
                || file_size.fract() != 0.0
                || filename_length == 0
                || filename_length > 255
            {
                return Err(String::from("invalid file size or filename length"));
            }
            let profile = u8::try_from(profile)
                .ok()
                .and_then(|p| ProfileId::try_from(p).ok())
                .ok_or_else(|| String::from("unsupported profile"))?;
            let name = std::str::from_utf8(&state.input[16..16 + filename_length as usize])
                .map_err(|_| String::from("filename is not UTF-8"))?
                .to_owned();
            prepare(
                name,
                file_size as u64,
                profile,
                state.input[..16].try_into().unwrap(),
                BrowserSource,
                |done, total| {
                    // Import only posts a progress message; it cannot reenter this RefCell.
                    unsafe { hash_progress(done as f64, total as f64) }
                },
            )
        })();
        match result {
            Ok(sender) => {
                state.digest = sender.manifest().sha256;
                state.sender = Some(sender);
                1
            }
            Err(error) => {
                state.error = error;
                0
            }
        }
    })
}

#[no_mangle]
pub extern "C" fn vft_next() -> i32 {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        state.frame.clear();
        let result = state
            .sender
            .as_mut()
            .ok_or_else(|| String::from("sender is not initialized"))
            .and_then(|sender| sender.next_frame().map_err(|e| e.to_string()));
        match result {
            Ok(frame) => {
                state.frame = frame.cells;
                state.error.clear();
                1
            }
            Err(error) => {
                state.error = error;
                0
            }
        }
    })
}

#[no_mangle]
pub extern "C" fn vft_frame_ptr() -> *const u8 {
    STATE.with(|s| s.borrow().frame.as_ptr())
}
#[no_mangle]
pub extern "C" fn vft_frame_len() -> u32 {
    STATE.with(|s| s.borrow().frame.len() as u32)
}
#[no_mangle]
pub extern "C" fn vft_error_ptr() -> *const u8 {
    STATE.with(|s| s.borrow().error.as_ptr())
}
#[no_mangle]
pub extern "C" fn vft_error_len() -> u32 {
    STATE.with(|s| s.borrow().error.len() as u32)
}
#[no_mangle]
pub extern "C" fn vft_digest_ptr() -> *const u8 {
    STATE.with(|s| s.borrow().digest.as_ptr())
}
/// 0=columns, 1=rows, 2=generated frames, 3=sources, 4=repairs, 5=total source symbols.
#[no_mangle]
pub extern "C" fn vft_stat(index: u32) -> f64 {
    STATE.with(|state| {
        let state = state.borrow();
        let Some(sender) = &state.sender else {
            return 0.0;
        };
        let grid = vft_visual::GridConfig::for_profile(sender.manifest().profile);
        let progress = sender.progress();
        match index {
            0 => grid.columns as f64,
            1 => grid.rows as f64,
            2 => progress.frames as f64,
            3 => progress.source_symbols as f64,
            4 => progress.repair_symbols as f64,
            5 => sender.manifest().source_symbols as f64,
            _ => 0.0,
        }
    })
}
