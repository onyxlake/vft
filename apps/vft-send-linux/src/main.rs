#[cfg(target_os = "linux")]
mod wayland;
fn main() {
    #[cfg(target_os = "linux")]
    if let Err(error) = wayland::run() {
        eprintln!("Sender failed: {}", vft_tui::safe_text(&error.to_string()));
        std::process::exit(1);
    }
    #[cfg(not(target_os = "linux"))]
    {
        eprintln!("vft-send-linux requires Linux Wayland");
        std::process::exit(1);
    }
}
