//! Offline Git synchronization in the std application layer. No visual/wire changes.
pub mod cli;
mod format;
mod interrupt;
mod process;
mod repository;
mod temporary;
pub use format::{Header, ObjectFormat, HEADER_LEN};
pub use interrupt::InterruptGuard;
pub use process::Control;
pub use repository::{prepare, ApplyOutcome, Prepared, ReceiveSession};
pub use temporary::TempDir;

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
pub const DEFAULT_MAX_PACKAGE: u64 = 256 * 1024 * 1024;
