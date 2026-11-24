#[cfg(any(windows, target_os = "linux"))]
mod git;
#[cfg(any(windows, target_os = "linux", test))]
#[path = "../../receiver_output.rs"]
mod output;
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
        eprintln!(
            "Receiver failed: {}",
            vft_tui::safe_text(&error.to_string())
        );
        std::process::exit(1);
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        eprintln!("vft-recv requires Windows 10/11 or Linux Wayland");
        std::process::exit(1);
    }
}

#[cfg(test)]
#[path = "../../vft-git/tests/common/mod.rs"]
mod git_fixture;
