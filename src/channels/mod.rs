//! Channel abstraction for message I/O.

pub mod channel;
pub mod cli;
pub mod manager;

pub use channel::*;
pub use cli::CliChannel;
pub use manager::ChannelManager;
