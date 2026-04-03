//! Per-connection WebSocket rate limiter.
//!
//! Sliding-window counter: tracks timestamps of recent messages and rejects
//! new ones when the window is full. No external crate required.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// A lightweight sliding-window rate limiter.
///
/// Designed for per-connection use inside a WebSocket handler task.
pub struct RateLimiter {
    /// Timestamps of messages within the current window.
    timestamps: VecDeque<Instant>,
    /// Length of the sliding window.
    window: Duration,
    /// Maximum number of messages allowed within `window`.
    max: usize,
}

impl RateLimiter {
    /// Create a rate limiter that allows `max` messages per `window`.
    pub fn new(max: usize, window: Duration) -> Self {
        Self {
            timestamps: VecDeque::with_capacity(max + 1),
            window,
            max,
        }
    }

    /// Create a rate limiter allowing `max` messages per second.
    pub fn per_second(max: usize) -> Self {
        Self::new(max, Duration::from_secs(1))
    }

    /// Check whether a new message is allowed. Returns `true` if within
    /// the limit, `false` if the message should be rejected.
    pub fn check(&mut self) -> bool {
        let now = Instant::now();

        // Evict entries outside the window
        while let Some(&front) = self.timestamps.front() {
            if now.duration_since(front) > self.window {
                self.timestamps.pop_front();
            } else {
                break;
            }
        }

        if self.timestamps.len() >= self.max {
            false
        } else {
            self.timestamps.push_back(now);
            true
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_up_to_max() {
        let mut rl = RateLimiter::per_second(3);
        assert!(rl.check());
        assert!(rl.check());
        assert!(rl.check());
        assert!(!rl.check());
    }

    #[test]
    fn window_expires() {
        let mut rl = RateLimiter::new(2, Duration::from_millis(50));
        assert!(rl.check());
        assert!(rl.check());
        assert!(!rl.check());

        std::thread::sleep(Duration::from_millis(60));

        assert!(rl.check());
        assert!(rl.check());
        assert!(!rl.check());
    }
}
