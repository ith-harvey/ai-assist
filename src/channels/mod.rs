//! Channel abstraction for message I/O.

pub mod channel;
pub mod cli;
pub mod email;
pub mod email_poller;
pub mod email_types;
pub mod ios;
pub mod manager;
pub mod telegram;
pub mod todo_channel;

pub use channel::*;
pub use cli::CliChannel;
// EmailChannel removed — email uses standalone pipeline (email_poller + email_processor).
pub use email_types::EmailMessage;
pub use ios::IosChannel;
pub use manager::ChannelManager;
pub use telegram::TelegramChannel;
pub use todo_channel::TodoChannel;
