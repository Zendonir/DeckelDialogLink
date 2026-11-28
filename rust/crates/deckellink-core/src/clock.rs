//! Time abstraction and cooperative cancellation.
//!
//! All transfer code obtains time through a [`Clock`], so timeouts are deterministic
//! in tests ([`FakeClock`]) and the simulator can run faster than real time.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::{Duration, Instant};

use chrono::{DateTime, FixedOffset, Local, TimeZone, Utc};

/// Thread-safe cancellation flag. Cloning shares the flag.
#[derive(Debug, Clone, Default)]
pub struct CancelToken {
    inner: Arc<CancelInner>,
}

#[derive(Debug, Default)]
struct CancelInner {
    flag: AtomicBool,
    lock: Mutex<()>,
    signal: Condvar,
}

impl CancelToken {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.inner.flag.store(true, Ordering::SeqCst);
        let _guard = self.inner.lock.lock().unwrap_or_else(PoisonError::into_inner);
        self.inner.signal.notify_all();
    }

    pub fn is_cancelled(&self) -> bool {
        self.inner.flag.load(Ordering::SeqCst)
    }

    /// Wait up to `timeout`; returns `true` if cancelled.
    pub fn wait(&self, timeout: Duration) -> bool {
        if self.is_cancelled() {
            return true;
        }
        let deadline = Instant::now() + timeout;
        let mut guard = self.inner.lock.lock().unwrap_or_else(PoisonError::into_inner);
        loop {
            if self.is_cancelled() {
                return true;
            }
            let now = Instant::now();
            if now >= deadline {
                return false;
            }
            let (next, _) =
                self.inner.signal.wait_timeout(guard, deadline - now).unwrap_or_else(PoisonError::into_inner);
            guard = next;
        }
    }
}

/// Source of monotonic time, wall-clock time and interruptible sleeps.
pub trait Clock: Send + Sync + std::fmt::Debug {
    /// Monotonic time in seconds.
    fn monotonic(&self) -> f64;
    /// Wall-clock time with offset.
    fn now(&self) -> DateTime<FixedOffset>;
    /// Sleep `seconds`; returns `true` if `cancel` was triggered.
    fn sleep(&self, seconds: f64, cancel: Option<&CancelToken>) -> bool;
}

fn duration(seconds: f64) -> Duration {
    if seconds.is_finite() && seconds > 0.0 {
        Duration::from_secs_f64(seconds.min(86_400.0 * 365.0))
    } else {
        Duration::ZERO
    }
}

/// Real time.
#[derive(Debug, Clone)]
pub struct SystemClock {
    origin: Instant,
}

impl Default for SystemClock {
    fn default() -> Self {
        Self { origin: Instant::now() }
    }
}

impl Clock for SystemClock {
    fn monotonic(&self) -> f64 {
        self.origin.elapsed().as_secs_f64()
    }

    fn now(&self) -> DateTime<FixedOffset> {
        Local::now().fixed_offset()
    }

    fn sleep(&self, seconds: f64, cancel: Option<&CancelToken>) -> bool {
        match cancel {
            Some(token) => token.wait(duration(seconds)),
            None => {
                std::thread::sleep(duration(seconds));
                false
            }
        }
    }
}

/// Real time running `scale` times faster (simulator).
#[derive(Debug, Clone)]
pub struct ScaledClock {
    origin: Instant,
    scale: f64,
}

impl ScaledClock {
    pub fn new(scale: f64) -> Self {
        let scale = if scale.is_finite() && scale > 0.0 { scale } else { 1.0 };
        Self { origin: Instant::now(), scale }
    }
}

impl Clock for ScaledClock {
    fn monotonic(&self) -> f64 {
        self.origin.elapsed().as_secs_f64() * self.scale
    }

    fn now(&self) -> DateTime<FixedOffset> {
        Local::now().fixed_offset()
    }

    fn sleep(&self, seconds: f64, cancel: Option<&CancelToken>) -> bool {
        SystemClock::default().sleep(seconds / self.scale, cancel)
    }
}

/// Virtual time that only advances on `sleep`/`advance` (deterministic tests).
#[derive(Debug, Clone)]
pub struct FakeClock {
    start: DateTime<FixedOffset>,
    elapsed: Arc<Mutex<f64>>,
}

impl Default for FakeClock {
    fn default() -> Self {
        let start = Utc
            .with_ymd_and_hms(2026, 1, 1, 8, 0, 0)
            .single()
            .map_or_else(|| Utc::now().fixed_offset(), |t| t.fixed_offset());
        Self { start, elapsed: Arc::new(Mutex::new(0.0)) }
    }
}

impl FakeClock {
    pub fn advance(&self, seconds: f64) {
        let mut elapsed = self.elapsed.lock().unwrap_or_else(PoisonError::into_inner);
        if seconds.is_finite() && seconds > 0.0 {
            *elapsed += seconds;
        }
    }
}

impl Clock for FakeClock {
    fn monotonic(&self) -> f64 {
        *self.elapsed.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn now(&self) -> DateTime<FixedOffset> {
        let elapsed = self.monotonic();
        #[allow(clippy::cast_possible_truncation)]
        let micros = (elapsed * 1e6) as i64;
        self.start + chrono::Duration::microseconds(micros)
    }

    fn sleep(&self, seconds: f64, cancel: Option<&CancelToken>) -> bool {
        if cancel.is_some_and(CancelToken::is_cancelled) {
            return true;
        }
        self.advance(seconds);
        cancel.is_some_and(CancelToken::is_cancelled)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fake_clock_advances_only_on_sleep() {
        let clock = FakeClock::default();
        assert_eq!(clock.monotonic(), 0.0);
        assert!(!clock.sleep(1.5, None));
        assert_eq!(clock.monotonic(), 1.5);
        clock.advance(-3.0);
        assert_eq!(clock.monotonic(), 1.5);
        let token = CancelToken::new();
        token.cancel();
        assert!(clock.sleep(1.0, Some(&token)));
        assert_eq!(clock.monotonic(), 1.5);
        assert_eq!((clock.now() - clock.start).num_milliseconds(), 1500);
    }

    #[test]
    fn cancel_wakes_waiting_thread() {
        let token = CancelToken::new();
        let other = token.clone();
        let handle = std::thread::spawn(move || other.wait(Duration::from_secs(30)));
        std::thread::sleep(Duration::from_millis(20));
        token.cancel();
        assert!(handle.join().unwrap());
        assert!(token.is_cancelled());
    }

    #[test]
    fn system_and_scaled_clock() {
        let clock = SystemClock::default();
        assert!(!clock.sleep(0.001, None));
        assert!(!clock.sleep(-1.0, Some(&CancelToken::new())));
        assert!(clock.monotonic() >= 0.0);
        let scaled = ScaledClock::new(1000.0);
        assert!(!scaled.sleep(1.0, None));
        assert!(scaled.monotonic() >= 0.0);
        assert!(ScaledClock::new(-1.0).scale > 0.0);
        let _ = clock.now();
        let _ = scaled.now();
    }
}
