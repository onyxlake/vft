#[cfg(windows)]
mod win;
fn main() {
    #[cfg(windows)]
    if let Err(error) = win::run() {
        eprintln!("Sender failed: {}", vft_tui::safe_text(&error.to_string()));
        std::process::exit(1);
    }
    #[cfg(not(windows))]
    {
        eprintln!("vft-send-win requires Windows 10/11");
        std::process::exit(1);
    }
}
