//! Terminal-independent control models; the terminal backend is Windows-only.
use std::{
    collections::{BTreeSet, VecDeque},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};
use vft_protocol::{
    repair_request::{RepairRequest, Target, MAX_BYTES, MAX_TARGETS},
    Manifest,
};
use vft_receiver::{GenerationProgress, ReceiverSession};
#[cfg(windows)]
pub mod terminal;

pub fn choose_mode(
    explicit: Option<bool>,
    stdin_terminal: bool,
    stdout_terminal: bool,
) -> Result<bool, &'static str> {
    let interactive = stdin_terminal && stdout_terminal;
    match explicit {
        Some(true) if !interactive => {
            Err("TUI requires interactive stdin and stdout; remove --tui or use --no-tui")
        }
        Some(value) => Ok(value),
        None => Ok(interactive),
    }
}
pub fn mode_flag(mode: &mut Option<bool>, value: bool) -> Result<(), &'static str> {
    if mode.is_some_and(|old| old != value) {
        return Err("--tui and --no-tui are mutually exclusive");
    }
    *mode = Some(value);
    Ok(())
}
pub fn safe_text(text: &str) -> String {
    text.chars()
        .flat_map(|c| {
            if c.is_control() {
                c.escape_default().collect::<Vec<_>>()
            } else {
                vec![c]
            }
        })
        .take(4096)
        .collect()
}
#[derive(Clone, Debug, Default)]
pub struct Editor {
    pub text: String,
    pub cursor: usize,
}
impl Editor {
    pub fn insert(&mut self, text: &str) -> Result<(), &'static str> {
        if text.chars().any(char::is_control) {
            return Err("Single-line input cannot contain control characters");
        }
        if self
            .text
            .len()
            .checked_add(text.len())
            .is_none_or(|n| n > MAX_BYTES)
        {
            return Err("Repair input is limited to 4096 bytes");
        }
        self.text.insert_str(self.cursor, text);
        self.cursor += text.len();
        Ok(())
    }
    pub fn left(&mut self) {
        self.cursor = self.text[..self.cursor]
            .char_indices()
            .last()
            .map_or(0, |(i, _)| i);
    }
    pub fn right(&mut self) {
        if let Some(c) = self.text[self.cursor..].chars().next() {
            self.cursor += c.len_utf8();
        }
    }
    pub fn backspace(&mut self) {
        let end = self.cursor;
        self.left();
        self.text.replace_range(self.cursor..end, "");
    }
    pub fn delete(&mut self) {
        let start = self.cursor;
        self.right();
        let end = self.cursor;
        self.cursor = start;
        self.text.replace_range(start..end, "");
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Lifecycle {
    #[default]
    Starting,
    Running,
    Paused,
    Stopping,
    Complete,
    Failed,
}
#[derive(Clone, Default)]
pub struct View {
    pub title: String,
    pub state: Lifecycle,
    pub details: Vec<String>,
    pub manifest: Option<Manifest>,
    pub groups: Vec<GenerationProgress>,
    pub page: usize,
    pub missing: usize,
    pub request: Option<RequestSnapshot>,
    pub applied: u64,
    pub result: String,
    pub worker_done: bool,
    pub logs: VecDeque<String>,
}
#[derive(Clone, Default)]
pub struct RequestSnapshot {
    pub generated: String,
    pub codes: Vec<String>,
    pub entries: Vec<String>,
}
impl View {
    pub fn log(&mut self, message: &str) {
        if self.logs.len() == 200 {
            self.logs.pop_front();
        }
        self.logs.push_back(safe_text(message));
    }
}
pub type SharedView = Arc<Mutex<View>>;
pub type Stop = Arc<AtomicBool>;
type PanicHook = Box<dyn Fn(&std::panic::PanicHookInfo<'_>) + Send + Sync + 'static>;
/// Keep the process-wide panic hook from writing over an active terminal. Callers catch unwind
/// and join workers before dropping this guard; the original hook is restored afterwards.
pub struct PanicLog {
    previous: Option<PanicHook>,
}
impl PanicLog {
    pub fn install(view: SharedView) -> Self {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            if let Ok(mut v) = view.try_lock() {
                v.log(&format!("Worker panic: {info}"));
            }
        }));
        Self {
            previous: Some(previous),
        }
    }
}
impl Drop for PanicLog {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            if let Some(previous) = self.previous.take() {
                std::panic::set_hook(previous);
            }
        }
    }
}
pub fn stopped(stop: &Stop) -> bool {
    stop.load(Ordering::Acquire)
}
pub fn stop(stop: &Stop) {
    stop.store(true, Ordering::Release);
}
pub const PAGE_SIZE: usize = 6;
/// Collect at most one screen of rows; completed/unseen data stays owned by the receiver.
pub fn update_groups(view: &mut View, receiver: &ReceiverSession, page: usize) {
    view.missing = receiver
        .generation_progress()
        .filter(|g| !g.complete)
        .count();
    view.page = page.min(view.missing.saturating_sub(1) / PAGE_SIZE);
    view.groups = receiver
        .generation_progress()
        .filter(|g| !g.complete)
        .skip(view.page * PAGE_SIZE)
        .take(PAGE_SIZE)
        .collect();
}
/// Freeze text at one capture boundary. Each code contains at most 256 targets.
pub fn generate_snapshot(
    receiver: &ReceiverSession,
    selected: &BTreeSet<u32>,
    mut id: impl FnMut() -> Result<u64, String>,
    generated: String,
) -> Result<RequestSnapshot, String> {
    let m = receiver
        .manifest()
        .ok_or("No manifest; wait for session discovery")?;
    if receiver.is_complete() {
        return Err(String::from("No incomplete groups remain"));
    }
    let mut snapshot = RequestSnapshot {
        generated,
        ..Default::default()
    };
    let mut targets = Vec::new();
    let emit = |targets: &mut Vec<Target>,
                snapshot: &mut RequestSnapshot,
                request_id: u64|
     -> Result<(), String> {
        let request = RepairRequest {
            id: Some(request_id),
            targets: std::mem::take(targets),
        };
        snapshot
            .codes
            .push(request.encode(m).map_err(|e| e.to_string())?);
        snapshot.entries.push(request.entries());
        Ok(())
    };
    for g in receiver
        .generation_progress()
        .filter(|g| !g.complete && (selected.is_empty() || selected.contains(&g.generation)))
    {
        targets.push(Target {
            generation: g.generation,
            deficit: Some(g.source_count - g.rank),
        });
        if targets.len() == MAX_TARGETS {
            emit(&mut targets, &mut snapshot, id()?)?;
        }
    }
    if !targets.is_empty() {
        emit(&mut targets, &mut snapshot, id()?)?;
    }
    if snapshot.codes.is_empty() {
        return Err(String::from(
            "Selected groups have already recovered; clear selection or refresh",
        ));
    }
    Ok(snapshot)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn request_pages_are_frozen_bounded_and_use_fresh_ids() {
        use vft_protocol::{Digest, Packet, PacketType, ProfileId, Sha256, SymbolId};
        let mut receiver = ReceiverSession::default();
        assert!(generate_snapshot(&receiver, &BTreeSet::new(), || Ok(1), String::new()).is_err());
        let m = Manifest {
            session: [1; 16],
            object: [2; 16],
            filename: String::from("中文.bin"),
            file_size: 64 * 300,
            sha256: Sha256::digest(vec![9; 64 * 300]).into(),
            symbol_size: 1,
            source_symbols: 64 * 300,
            profile: ProfileId::Balanced,
        };
        let p = Packet {
            session: m.session,
            object: m.object,
            profile: m.profile,
            kind: PacketType::Manifest,
            symbol: SymbolId {
                generation: 0,
                index: 0,
            },
            payload: m.encode().unwrap(),
        };
        receiver.push_packet(&p.encode().unwrap()).unwrap();
        let mut id = 0;
        let snapshot = generate_snapshot(
            &receiver,
            &BTreeSet::new(),
            || {
                id += 1;
                Ok(id)
            },
            String::from("test time"),
        )
        .unwrap();
        assert_eq!(snapshot.codes.len(), 2);
        assert_eq!(
            RepairRequest::parse(&snapshot.codes[0], &m)
                .unwrap()
                .targets
                .len(),
            256
        );
        assert_eq!(
            RepairRequest::parse(&snapshot.codes[1], &m)
                .unwrap()
                .targets
                .len(),
            44
        );
        let frozen = snapshot.codes.clone();
        let source = Packet {
            kind: PacketType::SourceSymbol,
            payload: vec![9],
            ..p
        };
        receiver.push_packet(&source.encode().unwrap()).unwrap();
        assert_eq!(snapshot.codes, frozen);
        let selection = BTreeSet::from([0, 299]);
        let r = generate_snapshot(&receiver, &selection, || Ok(3), String::from("later")).unwrap();
        let r = RepairRequest::parse(&r.codes[0], &m).unwrap();
        assert_eq!(r.targets[0].deficit, Some(63));
        let mut v = View::default();
        update_groups(&mut v, &receiver, 999);
        assert_eq!(v.groups.len(), PAGE_SIZE);
        assert_eq!(v.page, 49);
    }
    #[test]
    fn mode_matrix_and_safe_unicode_editor() {
        for input in [false, true] {
            for output in [false, true] {
                assert_eq!(choose_mode(None, input, output).unwrap(), input && output);
                assert!(!choose_mode(Some(false), input, output).unwrap());
                assert_eq!(
                    choose_mode(Some(true), input, output).is_ok(),
                    input && output
                );
            }
        }
        let mut mode = None;
        mode_flag(&mut mode, true).unwrap();
        assert!(mode_flag(&mut mode, false).is_err());
        let mut e = Editor::default();
        e.insert("中文1").unwrap();
        e.left();
        e.backspace();
        assert_eq!(e.text, "中1");
        e.cursor = 0;
        e.delete();
        assert_eq!(e.text, "1");
        e.insert("42,").unwrap();
        assert_eq!(e.text, "42,1");
        assert!(e.insert("\u{1b}[2J").is_err());
        assert!(e.insert(&"x".repeat(MAX_BYTES)).is_err());
        assert_eq!(safe_text("中\u{1b}\n"), "中\\u{1b}\\n");
    }
}
