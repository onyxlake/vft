#[cfg(any(windows, target_os = "linux"))]
#[path = "../../sender_git.rs"]
mod git;
#[cfg(any(windows, test))]
mod rect_file;
#[cfg(target_os = "linux")]
mod wayland;
#[cfg(windows)]
mod win;

#[cfg(target_os = "linux")]
use wayland::run;
#[cfg(windows)]
use win::run;

fn main() {
    #[cfg(any(windows, target_os = "linux"))]
    if let Err(error) = run() {
        eprintln!("Sender failed: {}", vft_tui::safe_text(&error.to_string()));
        std::process::exit(1);
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        eprintln!("vft-send requires Windows 10/11 or Linux Wayland");
        std::process::exit(1);
    }
}
