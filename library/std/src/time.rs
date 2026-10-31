// `std::time` (docs/ARCH.md section 3.1; W-STD-C): `Instant`,
// `SystemTime`, `UNIX_EPOCH`, `SystemTimeError`, and the re-exports of
// `core::time`.
//
// Lust std source, adapted from rust-lang/rust 1.99 `std/src/time.rs`
// and the unix `Timespec` (`std/src/sys/pal/unix/time.rs`). Spliced as a
// hidden module like library/core/src/time.rs (same rules); the clocks
// are the `clock_now_raw` and `system_time_raw` intrinsics.

pub use core::time::Duration;
pub use core::time::TryFromFloatSecsError;

const NANOS_PER_SEC: u32 = 1_000_000_000;

/// The unix `Timespec`: signed seconds and nanoseconds below one second.
/// Its derived Debug prints both fields, as std's does.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
struct Timespec {
    tv_sec: i64,
    tv_nsec: u32,
}

impl Timespec {
    const fn zero() -> Timespec {
        Timespec { tv_sec: 0, tv_nsec: 0 }
    }

    /// Reads a clock through its intrinsic: the low word of `raw` is
    /// `tv_sec` (as i64 bits), the high word is `tv_nsec`.
    fn from_raw(raw: u128) -> Timespec {
        Timespec { tv_sec: raw as u64 as i64, tv_nsec: (raw >> 64) as u32 }
    }

    fn sub_timespec(&self, other: &Timespec) -> Result<Duration, Duration> {
        if *self >= *other {
            let (secs, nsec) = if self.tv_nsec >= other.tv_nsec {
                (self.tv_sec.wrapping_sub(other.tv_sec) as u64, self.tv_nsec - other.tv_nsec)
            } else {
                ((self.tv_sec - 1).wrapping_sub(other.tv_sec) as u64, self.tv_nsec + NANOS_PER_SEC - other.tv_nsec)
            };
            Ok(Duration::new(secs, nsec))
        } else {
            match other.sub_timespec(self) {
                Ok(d) => Err(d),
                Err(d) => Ok(d),
            }
        }
    }

    fn checked_add_duration(&self, other: &Duration) -> Option<Timespec> {
        let mut secs = i64_add_unsigned(self.tv_sec, other.as_secs())?;
        let mut nsec = other.subsec_nanos() + self.tv_nsec;
        if nsec >= NANOS_PER_SEC {
            nsec -= NANOS_PER_SEC;
            secs = secs.checked_add(1)?;
        }
        Some(Timespec { tv_sec: secs, tv_nsec: nsec })
    }

    fn checked_sub_duration(&self, other: &Duration) -> Option<Timespec> {
        let mut secs = i64_sub_unsigned(self.tv_sec, other.as_secs())?;
        let mut nsec = self.tv_nsec as i32 - other.subsec_nanos() as i32;
        if nsec < 0 {
            nsec += NANOS_PER_SEC as i32;
            secs = secs.checked_sub(1)?;
        }
        Some(Timespec { tv_sec: secs, tv_nsec: nsec as u32 })
    }
}

/// `i64::checked_add_unsigned`.
fn i64_add_unsigned(a: i64, b: u64) -> Option<i64> {
    let r = a as i128 + b as i128;
    if r > i64::MAX as i128 { None } else { Some(r as i64) }
}

/// `i64::checked_sub_unsigned`.
fn i64_sub_unsigned(a: i64, b: u64) -> Option<i64> {
    let r = a as i128 - b as i128;
    if r < i64::MIN as i128 { None } else { Some(r as i64) }
}

/// A monotonic clock reading (`CLOCK_MONOTONIC`).
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Instant {
    t: Timespec,
}

impl Instant {
    pub fn now() -> Instant {
        Instant { t: Timespec::from_raw(core::intrinsics::clock_now_raw()) }
    }

    pub fn duration_since(&self, earlier: Instant) -> Duration {
        self.checked_duration_since(earlier).unwrap_or_default()
    }

    pub fn checked_duration_since(&self, earlier: Instant) -> Option<Duration> {
        // Windows Instant treats a difference within one QPC tick as zero
        // (std's `checked_sub_instant` epsilon). `start + 1ns` stays `start`
        // on the runners' counter, so this returns `Some(0ns)`.
        if cfg!(windows) && earlier.t > self.t {
            if let Ok(d) = earlier.t.sub_timespec(&self.t) {
                let freq = core::intrinsics::qpc_freq();
                let eps = if freq == 0 { 0 } else { 1_000_000_000u64 / freq };
                if d.as_secs() == 0 && (d.subsec_nanos() as u64) <= eps {
                    return Some(Duration::new(0, 0));
                }
            }
        }
        match self.t.sub_timespec(&earlier.t) {
            Ok(d) => Some(d),
            Err(_) => None,
        }
    }

    pub fn saturating_duration_since(&self, earlier: Instant) -> Duration {
        self.checked_duration_since(earlier).unwrap_or_default()
    }

    pub fn elapsed(&self) -> Duration {
        Instant::now() - *self
    }

    pub fn checked_add(&self, duration: Duration) -> Option<Instant> {
        match self.t.checked_add_duration(&duration) {
            Some(t) => Some(Instant { t }),
            None => None,
        }
    }

    pub fn checked_sub(&self, duration: Duration) -> Option<Instant> {
        match self.t.checked_sub_duration(&duration) {
            Some(t) => Some(Instant { t }),
            None => None,
        }
    }
}

impl core::ops::Add<Duration> for Instant {
    type Output = Instant;
    #[track_caller]
    fn add(self, other: Duration) -> Instant {
        self.checked_add(other).expect("overflow when adding duration to instant")
    }
}

impl core::ops::AddAssign<Duration> for Instant {
    fn add_assign(&mut self, other: Duration) {
        *self = *self + other; //@ std/src/time.rs:435:17
    }
}

impl core::ops::Sub<Duration> for Instant {
    type Output = Instant;
    #[track_caller]
    fn sub(self, other: Duration) -> Instant {
        self.checked_sub(other).expect("overflow when subtracting duration from instant")
    }
}

impl core::ops::SubAssign<Duration> for Instant {
    fn sub_assign(&mut self, other: Duration) {
        *self = *self - other; //@ std/src/time.rs:452:17
    }
}

impl core::ops::Sub<Instant> for Instant {
    type Output = Duration;
    fn sub(self, other: Instant) -> Duration {
        self.duration_since(other)
    }
}

impl core::fmt::Debug for Instant {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Instant").field("tv_sec", &self.t.tv_sec).field("tv_nsec", &self.t.tv_nsec).finish()
    }
}

/// A wall clock reading. Unix stores a `Timespec`. Windows stores 100 ns
/// intervals since 1601, which is what `Debug` prints.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SystemTime {
    #[cfg(not(windows))]
    t: Timespec,
    #[cfg(windows)]
    intervals: i64,
}

#[cfg(not(windows))]
pub const UNIX_EPOCH: SystemTime = SystemTime { t: Timespec::zero() };

/// 100 ns ticks in one second, and the Unix epoch as a `FILETIME` count.
#[cfg(windows)]
const INTERVALS_PER_SEC: u64 = 10_000_000;
#[cfg(windows)]
const UNIX_EPOCH_INTERVALS: i64 = 11_644_473_600i64 * INTERVALS_PER_SEC as i64;
#[cfg(windows)]
pub const UNIX_EPOCH: SystemTime = SystemTime { intervals: UNIX_EPOCH_INTERVALS };

#[cfg(not(windows))]
impl SystemTime {
    pub const UNIX_EPOCH: SystemTime = SystemTime { t: Timespec::zero() };

    pub fn now() -> SystemTime {
        SystemTime { t: Timespec::from_raw(core::intrinsics::system_time_raw()) }
    }

    pub fn duration_since(&self, earlier: SystemTime) -> Result<Duration, SystemTimeError> {
        match self.t.sub_timespec(&earlier.t) {
            Ok(d) => Ok(d),
            Err(d) => Err(SystemTimeError(d)),
        }
    }

    pub fn checked_add(&self, duration: Duration) -> Option<SystemTime> {
        match self.t.checked_add_duration(&duration) {
            Some(t) => Some(SystemTime { t }),
            None => None,
        }
    }

    pub fn checked_sub(&self, duration: Duration) -> Option<SystemTime> {
        match self.t.checked_sub_duration(&duration) {
            Some(t) => Some(SystemTime { t }),
            None => None,
        }
    }
}

#[cfg(windows)]
impl SystemTime {
    pub const UNIX_EPOCH: SystemTime = SystemTime { intervals: UNIX_EPOCH_INTERVALS };

    pub fn now() -> SystemTime {
        let t = Timespec::from_raw(core::intrinsics::system_time_raw());
        let intervals = (t.tv_sec as i128 + 11_644_473_600) * INTERVALS_PER_SEC as i128 + (t.tv_nsec / 100) as i128;
        SystemTime { intervals: intervals as i64 }
    }

    pub fn duration_since(&self, earlier: SystemTime) -> Result<Duration, SystemTimeError> {
        let (me, other) = (self.intervals, earlier.intervals);
        if me >= other { Ok(intervals_to_dur((me - other) as u64)) } else { Err(SystemTimeError(intervals_to_dur((other - me) as u64))) }
    }

    pub fn checked_add(&self, duration: Duration) -> Option<SystemTime> {
        Some(SystemTime { intervals: self.intervals.checked_add(dur_to_intervals(&duration)?)? })
    }

    pub fn checked_sub(&self, duration: Duration) -> Option<SystemTime> {
        // Windows rejects a time before 1601 (a negative interval).
        let intervals = self.intervals.checked_sub(dur_to_intervals(&duration)?)?;
        if intervals < 0 { None } else { Some(SystemTime { intervals }) }
    }
}

impl SystemTime {
    pub fn elapsed(&self) -> Result<Duration, SystemTimeError> {
        SystemTime::now().duration_since(*self)
    }
}

/// `Duration` as Windows 100 ns intervals, truncating leftover nanoseconds.
#[cfg(windows)]
fn dur_to_intervals(d: &Duration) -> Option<i64> {
    let n = d.as_secs().checked_mul(INTERVALS_PER_SEC)?.checked_add((d.subsec_nanos() / 100) as u64)?;
    if n > i64::MAX as u64 { None } else { Some(n as i64) }
}

/// Windows 100 ns intervals as a `Duration`.
#[cfg(windows)]
fn intervals_to_dur(n: u64) -> Duration {
    Duration::new(n / INTERVALS_PER_SEC, ((n % INTERVALS_PER_SEC) * 100) as u32)
}

impl core::ops::Add<Duration> for SystemTime {
    type Output = SystemTime;
    #[track_caller]
    fn add(self, dur: Duration) -> SystemTime {
        self.checked_add(dur).expect("overflow when adding duration to `SystemTime`")
    }
}

impl core::ops::AddAssign<Duration> for SystemTime {
    fn add_assign(&mut self, other: Duration) {
        *self = *self + other; //@ std/src/time.rs:754:17
    }
}

impl core::ops::Sub<Duration> for SystemTime {
    type Output = SystemTime;
    #[track_caller]
    fn sub(self, dur: Duration) -> SystemTime {
        self.checked_sub(dur).expect("overflow when subtracting duration from `SystemTime`")
    }
}

impl core::ops::SubAssign<Duration> for SystemTime {
    fn sub_assign(&mut self, other: Duration) {
        *self = *self - other; //@ std/src/time.rs:771:17
    }
}

#[cfg(not(windows))]
impl core::fmt::Debug for SystemTime {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("SystemTime").field("tv_sec", &self.t.tv_sec).field("tv_nsec", &self.t.tv_nsec).finish()
    }
}

#[cfg(windows)]
impl core::fmt::Debug for SystemTime {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("SystemTime").field("intervals", &self.intervals).finish()
    }
}

/// The error of `SystemTime::duration_since` when `earlier` is later.
#[derive(Clone, Debug)]
pub struct SystemTimeError(Duration);

impl SystemTimeError {
    pub fn duration(&self) -> Duration {
        self.0
    }
}

impl core::fmt::Display for SystemTimeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "second time provided was later than self")
    }
}
