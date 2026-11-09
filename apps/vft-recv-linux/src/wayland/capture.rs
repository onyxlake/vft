//! Portal session and PipeWire resources stay alive until capture has stopped.
use super::State;
use ashpd::desktop::{
    screencast::{CursorMode, Screencast, SourceType},
    PersistMode,
};
use pipewire::{self as pw, properties::properties, spa};
use std::{
    cell::RefCell,
    future::Future,
    os::fd::OwnedFd,
    rc::Rc,
    time::{Duration, Instant},
};
use vft_linux::{packed_luma, PixelOrder, Result};
use vft_visual::LumaImage;

async fn wait<F: Future>(state: &State, future: F) -> Result<F::Output> {
    tokio::pin!(future);
    loop {
        tokio::select! {
            result = &mut future => return Ok(result),
            _ = tokio::time::sleep(Duration::from_millis(100)) => {
                if vft_tui::stopped(&state.stop) { return Err("capture stopped during portal selection".into()); }
                if state.start.elapsed() >= Duration::from_secs(state.options.timeout) { return Err("capture timed out during portal selection".into()); }
            }
        }
    }
}
pub(super) fn run(state: &mut State) -> Result<()> {
    // A background Tokio worker keeps the portal's D-Bus connection serviced while the
    // calling thread runs PipeWire. No desktop is contacted until this function is called.
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()?;
    let portal = runtime.block_on(wait(state, Screencast::new()))??;
    let session = runtime.block_on(wait(state, portal.create_session()))??;
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let (node, fd) = runtime.block_on(wait(state, async {
            let types =
                portal.available_source_types().await? & (SourceType::Monitor | SourceType::Window);
            if types.is_empty() {
                return Err("portal offers neither monitor nor window capture".into());
            }
            let modes = portal.available_cursor_modes().await?;
            let cursor = if modes.contains(CursorMode::Hidden) {
                CursorMode::Hidden
            } else if modes.contains(CursorMode::Metadata) {
                CursorMode::Metadata
            } else if modes.contains(CursorMode::Embedded) {
                state.report("Portal embeds the cursor; keep it outside the visual frame.");
                CursorMode::Embedded
            } else {
                return Err("portal offers no supported cursor mode".into());
            };
            portal
                .select_sources(&session, cursor, types, false, None, PersistMode::DoNot)
                .await?
                .response()?;
            let response = portal.start(&session, None).await?.response()?;
            let stream = response
                .streams()
                .first()
                .ok_or("portal returned no capture stream")?;
            let fd = portal.open_pipe_wire_remote(&session).await?;
            Ok::<_, Box<dyn std::error::Error + Send + Sync>>((stream.pipe_wire_node_id(), fd))
        }))??;
        stream(state, node, fd)
    }))
    .unwrap_or_else(|_| Err("capture setup/loop panicked".into()));
    // Close on success, cancellation, setup failure and stream errors. Bound cleanup too.
    let close = runtime
        .block_on(async { tokio::time::timeout(Duration::from_secs(3), session.close()).await });
    match close {
        Ok(Ok(())) => {}
        Ok(Err(error)) => state.report(&format!("Portal session close failed: {error}")),
        Err(_) => state.report("Portal session close timed out"),
    }
    result
}
struct Frames {
    latest: Option<LumaImage>,
    error: Option<String>,
    format: spa::param::video::VideoInfoRaw,
}
/// Never allow a panic to unwind through PipeWire's C callbacks.
fn guard(frames: &Rc<RefCell<Frames>>, f: impl FnOnce() -> Result<()>) {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f))
        .unwrap_or_else(|_| Err("PipeWire callback panicked".into()));
    if let Err(error) = result {
        frames
            .borrow_mut()
            .error
            .get_or_insert_with(|| error.to_string());
    }
}
fn stream(state: &mut State, node: u32, fd: OwnedFd) -> Result<()> {
    pw::init();
    let mainloop = pw::main_loop::MainLoopRc::new(None)?;
    let context = pw::context::ContextRc::new(&mainloop, None)?;
    let core = context.connect_fd(fd, None)?;
    let frames = Rc::new(RefCell::new(Frames {
        latest: None,
        error: None,
        format: Default::default(),
    }));
    let errors = frames.clone();
    let _core_listener = core
        .add_listener_local()
        .error(move |_, _, _, message| {
            guard(&errors, || {
                Err(format!("PipeWire core error: {message}").into())
            });
        })
        .register();
    let stream = pw::stream::StreamBox::new(
        &core,
        "VFT screen receiver",
        properties! {
            *pw::keys::MEDIA_TYPE => "Video",
            *pw::keys::MEDIA_CATEGORY => "Capture",
            *pw::keys::MEDIA_ROLE => "Screen",
        },
    )?;
    let changes = frames.clone();
    let formats = frames.clone();
    let incoming = frames.clone();
    let _listener = stream
        .add_local_listener::<()>()
        .state_changed(move |_, _, old, new| {
            guard(&changes, || match new {
                pw::stream::StreamState::Error(message) => {
                    Err(format!("PipeWire stream error: {message}").into())
                }
                pw::stream::StreamState::Unconnected
                    if !matches!(old, pw::stream::StreamState::Unconnected) =>
                {
                    Err("PipeWire stream disconnected (portal may have been closed)".into())
                }
                _ => Ok(()),
            });
        })
        .param_changed(move |_, _, id, param| {
            guard(&formats, || {
                if id != spa::param::ParamType::Format.as_raw() {
                    return Ok(());
                }
                let Some(param) = param else {
                    return Ok(());
                };
                let mut f = spa::param::video::VideoInfoRaw::default();
                f.parse(param)?;
                let size = f.size();
                if size.width == 0 || size.height == 0 || size.width > 8192 || size.height > 8192 {
                    return Err("unsupported capture size; maximum 8192x8192 stream pixels".into());
                }
                order(f.format())?;
                let mut frames = formats.borrow_mut();
                frames.format = f;
                frames.latest = None;
                Ok(())
            });
        })
        .process(move |stream, _| {
            guard(&incoming, || {
                // Dequeue and requeue every available buffer; retain just the newest CPU image.
                while let Some(mut buffer) = stream.dequeue_buffer() {
                    let datas = buffer.datas_mut();
                    if datas.len() != 1 {
                        return Err("capture requires one CPU-mapped packed RGB plane".into());
                    }
                    let data = &mut datas[0];
                    if data.chunk().size() == 0
                        || data
                            .chunk()
                            .flags()
                            .contains(spa::buffer::ChunkFlags::CORRUPTED)
                    {
                        continue;
                    }
                    let offset = data.chunk().offset() as usize;
                    let length = data.chunk().size() as usize;
                    let stride = data.chunk().stride();
                    let f = incoming.borrow().format;
                    let mapped = data.data().ok_or(
                        "PipeWire buffer is not CPU-mapped; DMA-BUF-only capture is unsupported",
                    )?;
                    let bytes = mapped
                        .get(offset..offset.checked_add(length).ok_or("capture chunk overflow")?)
                        .ok_or("capture chunk outside mapped buffer")?;
                    let image = packed_luma(
                        bytes,
                        f.size().width as usize,
                        f.size().height as usize,
                        stride,
                        order(f.format())?,
                    )?;
                    incoming.borrow_mut().latest = Some(image);
                }
                Ok(())
            });
        })
        .register()?;
    let object = spa::pod::object!(
        spa::utils::SpaTypes::ObjectParamFormat,
        spa::param::ParamType::EnumFormat,
        spa::pod::property!(
            spa::param::format::FormatProperties::MediaType,
            Id,
            spa::param::format::MediaType::Video
        ),
        spa::pod::property!(
            spa::param::format::FormatProperties::MediaSubtype,
            Id,
            spa::param::format::MediaSubtype::Raw
        ),
        spa::pod::property!(
            spa::param::format::FormatProperties::VideoFormat,
            Choice,
            Enum,
            Id,
            spa::param::video::VideoFormat::BGRx,
            spa::param::video::VideoFormat::BGRx,
            spa::param::video::VideoFormat::BGRA,
            spa::param::video::VideoFormat::RGBx,
            spa::param::video::VideoFormat::RGBA
        ),
        spa::pod::property!(
            spa::param::format::FormatProperties::VideoSize,
            Choice,
            Range,
            Rectangle,
            spa::utils::Rectangle {
                width: 1920,
                height: 1080
            },
            spa::utils::Rectangle {
                width: 1,
                height: 1
            },
            spa::utils::Rectangle {
                width: 8192,
                height: 8192
            }
        ),
        spa::pod::property!(
            spa::param::format::FormatProperties::VideoFramerate,
            Choice,
            Range,
            Fraction,
            spa::utils::Fraction {
                num: state.options.fps,
                denom: 1
            },
            spa::utils::Fraction { num: 0, denom: 1 },
            spa::utils::Fraction { num: 120, denom: 1 }
        )
    );
    let bytes = spa::pod::serialize::PodSerializer::serialize(
        std::io::Cursor::new(Vec::new()),
        &spa::pod::Value::Object(object),
    )?
    .0
    .into_inner();
    let mut params = [spa::pod::Pod::from_bytes(&bytes).ok_or("invalid PipeWire format pod")?];
    stream.connect(
        spa::utils::Direction::Input,
        Some(node),
        pw::stream::StreamFlags::AUTOCONNECT | pw::stream::StreamFlags::MAP_BUFFERS,
        &mut params,
    )?;
    let mut next = Instant::now();
    loop {
        if mainloop.loop_().iterate(Duration::from_millis(10)) < 0 {
            return Err("PipeWire loop iteration failed".into());
        }
        if let Some(error) = frames.borrow_mut().error.take() {
            return Err(error.into());
        }
        // FEC/hash/file IO happens after returning to Rust from all C callbacks.
        // Retain at most one image between iterations; stale frames are replaced.
        let now = Instant::now();
        let image = if now >= next {
            next = now + Duration::from_secs_f64(1.0 / f64::from(state.options.fps));
            frames.borrow_mut().latest.take()
        } else {
            None
        };
        if state.tick(image)? {
            return Ok(());
        }
    }
}

fn order(format: spa::param::video::VideoFormat) -> Result<PixelOrder> {
    use spa::param::video::VideoFormat as F;
    match format {
        F::BGRx | F::BGRA => Ok(PixelOrder::Bgr),
        F::RGBx | F::RGBA => Ok(PixelOrder::Rgb),
        _ => Err("unsupported capture pixel format".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn callback_errors_and_panics_are_contained() {
        let frames = Rc::new(RefCell::new(Frames {
            latest: None,
            error: None,
            format: Default::default(),
        }));
        guard(&frames, || Err("bad negotiated buffer".into()));
        assert_eq!(
            frames.borrow_mut().error.take().unwrap(),
            "bad negotiated buffer"
        );
        guard(&frames, || panic!("simulated callback panic"));
        assert!(frames
            .borrow()
            .error
            .as_ref()
            .unwrap()
            .contains("callback panicked"));
    }
    #[test]
    fn portal_wait_can_be_stopped_without_a_desktop_connection() {
        use std::sync::{atomic::AtomicBool, mpsc, Arc, Mutex};
        let now = Instant::now();
        let (_, commands) = mpsc::sync_channel(32);
        let options = super::super::Options {
            rect: None,
            search: None,
            output: crate::output::OutputDestination::explicit("unused".into()),
            preview: None,
            stats: None,
            profile: None,
            fps: 30,
            timeout: 1,
            max_file_size: 0,
            tui: Some(false),
        };
        let mut state = State {
            options,
            decoder: vft_linux::Decoder::new(None, None, None, 0),
            start: now,
            last_report: now,
            captures: 0,
            valid: 0,
            preview_saved: false,
            shared: Arc::new(Mutex::new(vft_tui::View::default())),
            stop: Arc::new(AtomicBool::new(true)),
            commands,
            page: 0,
        };
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .unwrap();
        let result = runtime.block_on(wait(&state, std::future::pending::<()>()));
        assert!(result.unwrap_err().to_string().contains("stopped"));
        state.stop = Arc::new(AtomicBool::new(false));
        state.start = now - Duration::from_secs(2);
        let result = runtime.block_on(wait(&state, std::future::pending::<()>()));
        assert!(result.unwrap_err().to_string().contains("timed out"));
    }
}
