//! One terminal owner calls both poll/read. Drop restores even partial setup and unwinding.
use crate::*;
use crossterm::{
    cursor::{Hide, Show},
    event::{
        self, DisableBracketedPaste, EnableBracketedPaste, Event, KeyCode, KeyEventKind,
        KeyModifiers,
    },
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Layout},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame, Terminal,
};
use std::{
    collections::BTreeSet,
    io::{self, Stdout},
    sync::mpsc::{SyncSender, TrySendError},
    time::Duration,
};
use vft_protocol::repair_request::{manifest_tag, RepairRequest};

pub enum SenderCommand {
    TogglePause,
    Cancel,
    Apply { serial: u64, request: RepairRequest },
}
pub enum ReceiverCommand {
    Page(usize),
    Generate(BTreeSet<u32>),
}
pub struct Screen {
    terminal: Terminal<CrosstermBackend<Stdout>>,
}
struct Restore;
impl Drop for Restore {
    fn drop(&mut self) {
        let _ = execute!(
            io::stdout(),
            DisableBracketedPaste,
            LeaveAlternateScreen,
            Show
        );
        let _ = disable_raw_mode();
    }
}
impl Screen {
    fn open() -> io::Result<(Self, Restore)> {
        enable_raw_mode()?;
        let guard = Restore;
        execute!(
            io::stdout(),
            EnterAlternateScreen,
            EnableBracketedPaste,
            Hide
        )?;
        let terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
        Ok((Self { terminal }, guard))
    }
}
fn boxed(frame: &mut Frame, area: ratatui::layout::Rect, title: &str, text: String, scroll: u16) {
    frame.render_widget(
        Paragraph::new(text)
            .block(Block::default().title(title).borders(Borders::ALL))
            .wrap(Wrap { trim: false })
            .scroll((scroll, 0)),
        area,
    );
}
fn draw(frame: &mut Frame, v: &View, body: String, help: &str, notice: &str, scroll: u16) {
    let area = frame.area();
    if area.width < 80 || area.height < 24 {
        frame.render_widget(Paragraph::new("Terminal too small: resize to at least 80x24. Q / Ctrl+C exits; Space pauses sender.").wrap(Wrap {trim:false}),area);
        return;
    }
    let rows = Layout::vertical([
        Constraint::Length(if area.width >= 100 && area.height >= 28 {
            8
        } else {
            7
        }),
        Constraint::Min(8),
        Constraint::Length(4),
        Constraint::Length(3),
    ])
    .split(area);
    boxed(
        frame,
        rows[0],
        &safe_text(&v.title),
        format!(
            "State: {:?}\n{}",
            v.state,
            v.details
                .iter()
                .map(|s| safe_text(s))
                .collect::<Vec<_>>()
                .join("\n")
        ),
        0,
    );
    boxed(frame, rows[1], "Controls / Progress", body, scroll);
    boxed(frame, rows[2], "Keys", help.to_string(), 0);
    boxed(
        frame,
        rows[3],
        "Status",
        if notice.is_empty() {
            v.logs.back().cloned().unwrap_or_default()
        } else {
            safe_text(notice)
        },
        0,
    );
}
fn send<T>(tx: &SyncSender<T>, cmd: T) -> Result<(), String> {
    tx.try_send(cmd).map_err(|e| match e {
        TrySendError::Full(_) => String::from("Command queue busy; try again"),
        TrySendError::Disconnected(_) => String::from("Session has ended"),
    })
}
fn quit(key: &crossterm::event::KeyEvent) -> bool {
    key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL)
}
fn next_event() -> io::Result<Option<Event>> {
    if event::poll(Duration::from_millis(200))? {
        Ok(Some(event::read()?))
    } else {
        Ok(None)
    }
}
pub fn random_id() -> Result<u64, String> {
    let mut bytes = [0u8; 8];
    getrandom::getrandom(&mut bytes).map_err(|e| format!("OS random request ID failed: {e}"))?;
    Ok(u64::from_le_bytes(bytes))
}

pub fn sender_ui(
    shared: SharedView,
    cancel: Stop,
    tx: SyncSender<SenderCommand>,
    wake: impl Fn(),
) -> io::Result<()> {
    let (mut screen, _restore) = Screen::open()?;
    let mut editor = Editor::default();
    let mut editing = false;
    let mut preview: Option<RepairRequest> = None;
    let mut notice = String::new();
    let mut serial = 0u64;
    let mut pending = None;
    let mut scroll = 0;
    loop {
        let v = shared.lock().unwrap().clone();
        if v.worker_done {
            break;
        }
        if pending.is_some_and(|id| v.applied == id) {
            pending = None;
            notice = v.result.clone();
            if v.result.starts_with("Request applied") || v.result.starts_with("Duplicate request")
            {
                editing = false;
                preview = None;
                editor = Editor::default();
            }
        }
        let body = if editing {
            let start = editor.text[..editor.cursor]
                .char_indices()
                .rev()
                .nth(24)
                .map_or(0, |(i, _)| i);
            let left = &editor.text[start..editor.cursor];
            let right = editor.text[editor.cursor..]
                .chars()
                .take(24)
                .collect::<String>();
            format!("Request: {left}|{right}\n{}",preview.as_ref().map_or(String::from("Enter validates; Esc cancels editing. Transmission continues."),|r|format!("Preview: {} groups, {} new repair packets\nFile: {}\nTag: {}\nTargets: {}\nEnter again applies. Up/Down scroll preview.",r.targets.len(),r.total_budget().unwrap_or(0),v.manifest.as_ref().map_or(String::from("test pattern"),|m|safe_text(&m.filename)),v.manifest.as_ref().and_then(|m|manifest_tag(m).ok()).unwrap_or_default(),r.entries())))
        } else {
            String::from("R enters a repair request.\nSimple: 18,42,90-91 or 18:3,42:1\nFull: VFR1/tag/request-id/entries/crc\nKeep this terminal outside the visual window.\nManual completion means the batch was sent; it does not indicate receiver completion.")
        };
        screen.terminal.draw(|f| {
            draw(
                f,
                &v,
                body,
                if editing {
                    "Enter: preview/apply  Esc: close  Arrows/Home/End/Backspace: edit"
                } else {
                    "R: request  Space: pause/resume  X: cancel manual  Q: quit"
                },
                &notice,
                scroll,
            )
        })?;
        match next_event()? {
            Some(Event::Key(k)) if k.kind != KeyEventKind::Release => {
                if quit(&k) {
                    stop(&cancel);
                    wake();
                    break;
                }
                if editing {
                    match k.code {
                        KeyCode::Esc => {
                            editing = false;
                            preview = None;
                            scroll = 0;
                        }
                        KeyCode::Enter if pending.is_none() => {
                            if let Some(request) = preview.clone() {
                                serial += 1;
                                match send(&tx, SenderCommand::Apply { serial, request }) {
                                    Ok(()) => {
                                        pending = Some(serial);
                                        notice = String::from("Submitting request...");
                                        wake();
                                    }
                                    Err(e) => notice = e,
                                }
                            } else if let Some(m) = &v.manifest {
                                match RepairRequest::parse(&editor.text, m) {
                                    Ok(r) => {
                                        preview = Some(r);
                                        notice = String::from(
                                            "Review targets and budget, then press Enter to apply",
                                        );
                                    }
                                    Err(e) => notice = e.to_string(),
                                }
                            } else {
                                notice = String::from(
                                    "No file session; test pattern cannot accept requests",
                                );
                            }
                        }
                        KeyCode::Left => editor.left(),
                        KeyCode::Right => editor.right(),
                        KeyCode::Home => editor.cursor = 0,
                        KeyCode::End => editor.cursor = editor.text.len(),
                        KeyCode::Up => scroll = scroll.saturating_sub(1),
                        KeyCode::Down => scroll = scroll.saturating_add(1),
                        KeyCode::Backspace if pending.is_none() => {
                            editor.backspace();
                            preview = None;
                            scroll = 0;
                        }
                        KeyCode::Delete if pending.is_none() => {
                            editor.delete();
                            preview = None;
                            scroll = 0;
                        }
                        KeyCode::Char(c) if pending.is_none() => {
                            if let Err(e) = editor.insert(&c.to_string()) {
                                notice = e.to_string();
                            }
                            preview = None;
                            scroll = 0;
                        }
                        _ => {}
                    }
                } else {
                    let cmd = match k.code {
                        KeyCode::Char('q' | 'Q') => {
                            stop(&cancel);
                            wake();
                            break;
                        }
                        KeyCode::Char('r' | 'R') => {
                            editing = true;
                            None
                        }
                        KeyCode::Char(' ') => Some(SenderCommand::TogglePause),
                        KeyCode::Char('x' | 'X') => Some(SenderCommand::Cancel),
                        _ => None,
                    };
                    if let Some(cmd) = cmd {
                        match send(&tx, cmd) {
                            Ok(()) => wake(),
                            Err(e) => notice = e,
                        }
                    }
                }
            }
            Some(Event::Paste(text)) if editing && pending.is_none() => {
                match editor.insert(&text) {
                    Ok(()) => {
                        preview = None;
                        scroll = 0;
                    }
                    Err(e) => notice = e.to_string(),
                }
            }
            _ => {}
        }
    }
    Ok(())
}
pub fn receiver_ui(
    shared: SharedView,
    cancel: Stop,
    tx: SyncSender<ReceiverCommand>,
) -> io::Result<()> {
    let (mut screen, _restore) = Screen::open()?;
    let mut selected = BTreeSet::new();
    let mut cursor = 0usize;
    let mut notice = String::new();
    let mut panel = false;
    let mut code_page = 0usize;
    let mut scroll = 0u16;
    let mut generation = String::new();
    loop {
        let v = shared.lock().unwrap().clone();
        cursor = cursor.min(v.groups.len().saturating_sub(1));
        if let Some(request) = &v.request {
            if request.generated != generation {
                generation = request.generated.clone();
                panel = true;
                code_page = 0;
                scroll = 0;
            }
        }
        let body = if panel {
            v.request.as_ref().map_or(String::from("Generating request..."),|r|format!("Frozen progress snapshot at {}. Recovery continues.\nRequest page {} / {}\nGroups / rank deficits: {}\n\n{}",r.generated,code_page+1,r.codes.len(),r.entries[code_page],r.codes[code_page]))
        } else {
            let mut text=format!("Incomplete groups: {}  Page {} / {}  Selected: {}\nGroup      Rank / Total    Deficit\n",v.missing,v.page+1,v.missing.div_ceil(PAGE_SIZE).max(1),selected.len());
            if v.worker_done {
                text = format!(
                    "{}\n\nQ / Ctrl+C exits. Up/Down scrolls this result.",
                    safe_text(&v.result)
                );
            }
            for (i, g) in v.groups.iter().enumerate() {
                text.push_str(&format!(
                    "{}{} {:<8} {:>2} / {:<2}          {}\n",
                    if i == cursor { ">" } else { " " },
                    if selected.contains(&g.generation) {
                        "*"
                    } else {
                        " "
                    },
                    g.generation + 1,
                    g.rank,
                    g.source_count,
                    g.source_count - g.rank
                ));
            }
            text
        };
        screen.terminal.draw(|f|draw(f,&v,body,if panel {"Tab: table  Left/Right: request page  Up/Down/PgUp/PgDn: scroll\nR: refresh  Q / Ctrl+C: quit"} else {"R: request  Arrows: navigate  Space: select  A: page  C: clear\nPgUp/PgDn: page  Tab: request/table  Q / Ctrl+C: quit"},&notice,scroll))?;
        if let Some(Event::Key(k)) = next_event()? {
            if k.kind == KeyEventKind::Release {
                continue;
            }
            if quit(&k) || matches!(k.code, KeyCode::Char('q' | 'Q')) {
                stop(&cancel);
                break;
            }
            let cmd = match k.code {
                KeyCode::Char('r' | 'R') => Some(ReceiverCommand::Generate(selected.clone())),
                KeyCode::Tab => {
                    panel = !panel;
                    scroll = 0;
                    None
                }
                KeyCode::Left if panel => {
                    code_page = code_page.saturating_sub(1);
                    scroll = 0;
                    None
                }
                KeyCode::Right if panel => {
                    if let Some(r) = &v.request {
                        code_page = (code_page + 1).min(r.codes.len() - 1);
                    }
                    scroll = 0;
                    None
                }
                KeyCode::Up if panel || v.worker_done => {
                    scroll = scroll.saturating_sub(1);
                    None
                }
                KeyCode::Down if panel || v.worker_done => {
                    scroll = scroll.saturating_add(1);
                    None
                }
                KeyCode::PageUp if panel || v.worker_done => {
                    scroll = scroll.saturating_sub(8);
                    None
                }
                KeyCode::PageDown if panel || v.worker_done => {
                    scroll = scroll.saturating_add(8);
                    None
                }
                KeyCode::Up => {
                    cursor = cursor.saturating_sub(1);
                    None
                }
                KeyCode::Down => {
                    cursor = (cursor + 1).min(v.groups.len().saturating_sub(1));
                    None
                }
                KeyCode::PageUp => Some(ReceiverCommand::Page(v.page.saturating_sub(1))),
                KeyCode::PageDown => Some(ReceiverCommand::Page(v.page + 1)),
                KeyCode::Char('c' | 'C') => {
                    selected.clear();
                    None
                }
                KeyCode::Char(' ') => {
                    if let Some(g) = v.groups.get(cursor) {
                        if !selected.remove(&g.generation) && selected.len() < MAX_TARGETS {
                            selected.insert(g.generation);
                        }
                    }
                    None
                }
                KeyCode::Char('a' | 'A') => {
                    for g in &v.groups {
                        if selected.len() < MAX_TARGETS {
                            selected.insert(g.generation);
                        }
                    }
                    None
                }
                _ => None,
            };
            if let Some(cmd) = cmd {
                notice = send(&tx, cmd).err().unwrap_or_default();
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    #[test]
    fn queue_busy_and_ended_results_are_visible_errors() {
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        send(&tx, 1).unwrap();
        assert_eq!(send(&tx, 2).unwrap_err(), "Command queue busy; try again");
        drop(rx);
        assert_eq!(send(&tx, 3).unwrap_err(), "Session has ended");
    }
    #[test]
    fn buffer_keeps_english_status_progress_and_small_terminal_exit() {
        for (width, height) in [(100, 28), (80, 24), (40, 10)] {
            let mut t = Terminal::new(TestBackend::new(width, height)).unwrap();
            let v = View {
                title: String::from("VFT Receive 中文.bin"),
                state: Lifecycle::Running,
                details: vec![String::from("SHA: Pending")],
                ..Default::default()
            };
            t.draw(|f| {
                draw(
                    f,
                    &v,
                    String::from("Group 18   61 / 64   Deficit 3"),
                    "Q: quit",
                    "Request applied",
                    0,
                )
            })
            .unwrap();
            let text = t
                .backend()
                .buffer()
                .content
                .iter()
                .map(|c| c.symbol())
                .collect::<String>();
            if width >= 80 {
                assert!(text.contains("SHA: Pending"));
                assert!(text.contains("Deficit 3"));
                assert!(text.contains("Request applied"));
            } else {
                assert!(text.contains("Ctrl+C exits"));
            }
        }
    }
}
