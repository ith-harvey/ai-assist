//! Channel abstraction for message I/O.

pub mod channel;
pub mod cli;
pub mod email;
pub mod email_types;
pub mod manager;
pub mod telegram;

pub use channel::*;
pub use cli::CliChannel;
pub use email::EmailChannel;
pub use email_types::EmailMessage;
pub use manager::ChannelManager;
pub use telegram::TelegramChannel;
