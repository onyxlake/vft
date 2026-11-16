//! CLI interrupts request cancellation through the same flag as the TUI. The branch-update
//! phase uses a different flag so Ctrl+C waits for Git and the final state inspection.
use crate::Result;
use std::sync::{atomic::AtomicBool, Arc};

pub struct InterruptGuard {
    #[cfg(windows)]
    registered: bool,
    #[cfg(unix)]
    previous: Vec<(libc::c_int, libc::sigaction)>,
    #[cfg(unix)]
    _exclusive: std::sync::MutexGuard<'static, ()>,
    #[cfg(unix)]
    stop: Arc<AtomicBool>,
    #[cfg(unix)]
    worker: Option<std::thread::JoinHandle<()>>,
}
impl InterruptGuard {
    pub fn install(cancel: Arc<AtomicBool>) -> Result<Self> {
        #[cfg(unix)]
        {
            use std::sync::atomic::Ordering;
            let exclusive = EXCLUSIVE
                .try_lock()
                .map_err(|_| "CLI interrupt handler already installed")?;
            INTERRUPTED.store(false, Ordering::Release);
            let stop = Arc::new(AtomicBool::new(false));
            let mut guard = Self {
                previous: Vec::new(),
                _exclusive: exclusive,
                stop: stop.clone(),
                worker: None,
            };
            for signal in [libc::SIGINT, libc::SIGTERM] {
                // sigaction is a C POD struct. The static handler touches only a lock-free
                // atomic, never allocation, locks or Arc ownership in asynchronous context.
                unsafe {
                    let mut action: libc::sigaction = std::mem::zeroed();
                    let mut previous: libc::sigaction = std::mem::zeroed();
                    action.sa_sigaction = unix_handler as *const () as usize;
                    action.sa_flags = libc::SA_RESTART;
                    libc::sigemptyset(&mut action.sa_mask);
                    if libc::sigaction(signal, &action, &mut previous) != 0 {
                        return Err(std::io::Error::last_os_error().into());
                    }
                    guard.previous.push((signal, previous));
                }
            }
            guard.worker = Some(
                std::thread::Builder::new()
                    .name("vft-git-interrupt".into())
                    .spawn(move || {
                        while !stop.load(Ordering::Acquire) {
                            if INTERRUPTED.swap(false, Ordering::AcqRel) {
                                cancel.store(true, Ordering::Release);
                            }
                            std::thread::sleep(std::time::Duration::from_millis(10));
                        }
                    })?,
            );
            Ok(guard)
        }
        #[cfg(windows)]
        {
            // A redirected/background process may have no console and cannot receive
            // console control events; it must still support offline CLI application.
            if unsafe { windows::Win32::System::Console::GetConsoleCP() } == 0 {
                return Ok(Self { registered: false });
            }
            let mut target = TARGET.lock().map_err(|_| "CLI interrupt lock poisoned")?;
            if target.is_some() {
                return Err("CLI interrupt handler already installed".into());
            }
            // Win32 invokes this static callback on its own thread; TARGET owns the Arc.
            unsafe {
                windows::Win32::System::Console::SetConsoleCtrlHandler(
                    Some(windows_handler),
                    true,
                )?;
            }
            *target = Some(cancel);
            Ok(Self { registered: true })
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = cancel;
            Ok(Self {})
        }
    }
}
impl Drop for InterruptGuard {
    fn drop(&mut self) {
        #[cfg(unix)]
        {
            // Restore precisely the actions we replaced, before releasing exclusive ownership.
            // Handler data is static and remains valid if a callback was already in flight.
            for (signal, action) in self.previous.iter().rev() {
                unsafe {
                    libc::sigaction(*signal, action, std::ptr::null_mut());
                }
            }
            self.stop.store(true, std::sync::atomic::Ordering::Release);
            if let Some(worker) = self.worker.take() {
                let _ = worker.join();
            }
        }
        #[cfg(windows)]
        {
            if !self.registered {
                return;
            }
            // Remove only our handler, leaving other console handlers intact.
            unsafe {
                let _ = windows::Win32::System::Console::SetConsoleCtrlHandler(
                    Some(windows_handler),
                    false,
                );
            }
            if let Ok(mut target) = TARGET.lock() {
                *target = None;
            }
        }
    }
}
#[cfg(unix)]
static EXCLUSIVE: std::sync::Mutex<()> = std::sync::Mutex::new(());
#[cfg(unix)]
static INTERRUPTED: AtomicBool = AtomicBool::new(false);
#[cfg(unix)]
extern "C" fn unix_handler(_: libc::c_int) {
    INTERRUPTED.store(true, std::sync::atomic::Ordering::Release);
}
#[cfg(windows)]
static TARGET: std::sync::Mutex<Option<Arc<AtomicBool>>> = std::sync::Mutex::new(None);
#[cfg(windows)]
unsafe extern "system" fn windows_handler(event: u32) -> windows::core::BOOL {
    use std::sync::atomic::Ordering;
    use windows::Win32::System::Console::{CTRL_BREAK_EVENT, CTRL_C_EVENT};
    // No panic crosses the FFI boundary. Poison is handled without unwrap; Windows console
    // callbacks are ordinary threads, not POSIX asynchronous signal handlers.
    if event == CTRL_C_EVENT || event == CTRL_BREAK_EVENT {
        if let Ok(target) = TARGET.lock() {
            if let Some(cancel) = target.as_ref() {
                cancel.store(true, Ordering::Release);
                return true.into();
            }
        }
    }
    false.into()
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering;
    #[test]
    fn cli_interrupts_cancel_and_restore_previous_signal_actions() {
        // Only this unit test installs process-wide handlers. Integration repository tests
        // run in separate test executables, so their process supervision is unaffected.
        for signal in [libc::SIGINT, libc::SIGTERM] {
            let mut before: libc::sigaction = unsafe { std::mem::zeroed() };
            assert_eq!(
                unsafe { libc::sigaction(signal, std::ptr::null(), &mut before) },
                0
            );
            let cancel = Arc::new(AtomicBool::new(false));
            let guard = InterruptGuard::install(cancel.clone()).unwrap();
            assert_eq!(unsafe { libc::raise(signal) }, 0);
            let start = std::time::Instant::now();
            while !cancel.load(Ordering::Acquire)
                && start.elapsed() < std::time::Duration::from_secs(2)
            {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            assert!(cancel.load(Ordering::Acquire));
            drop(guard);
            let mut after: libc::sigaction = unsafe { std::mem::zeroed() };
            assert_eq!(
                unsafe { libc::sigaction(signal, std::ptr::null(), &mut after) },
                0
            );
            assert_eq!(before.sa_sigaction, after.sa_sigaction);
        }
    }
}
