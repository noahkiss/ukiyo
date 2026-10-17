//! Time for the connection owner. Tests use `FakeClock`, where sleeping moves time forward
//! at once, so a 30 s reconnect backoff runs in microseconds.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub trait Clock: Send + Sync {
    /// Time since the clock started.
    fn now(&self) -> Duration;
    fn sleep(&self, d: Duration);
    /// Wall-clock milliseconds since the Unix epoch, for timestamps people read.
    fn wall_ms(&self) -> i64;
}

pub struct RealClock(Instant);

impl RealClock {
    pub fn new() -> Self {
        Self(Instant::now())
    }
}

impl Default for RealClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for RealClock {
    fn now(&self) -> Duration {
        self.0.elapsed()
    }
    fn sleep(&self, d: Duration) {
        std::thread::sleep(d)
    }
    fn wall_ms(&self) -> i64 {
        SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
    }
}

#[derive(Clone, Default)]
pub struct FakeClock(Arc<Mutex<Duration>>);

impl FakeClock {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn advance(&self, d: Duration) {
        *self.0.lock().unwrap() += d;
    }
}

impl Clock for FakeClock {
    fn now(&self) -> Duration {
        *self.0.lock().unwrap()
    }
    fn sleep(&self, d: Duration) {
        self.advance(d)
    }
    fn wall_ms(&self) -> i64 {
        1_700_000_000_000 + self.now().as_millis() as i64
    }
}
