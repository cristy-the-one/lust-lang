//! One check at a time, with a debounce in front of each check.
//!
//! Relevant events while idle collapse into a single deadline. Events while a
//! check is running do not start another check: they set one pending bit, and
//! the finish of that check arms exactly one follow-up (which itself debounces
//! with any edits that arrive after the check).

use std::time::{Duration, Instant};

#[derive(Debug)]
pub struct Scheduler {
    debounce: Duration,
    running: bool,
    pending: bool,
    deadline: Option<Instant>,
    last_event: Option<Instant>,
}

impl Scheduler {
    pub fn new(debounce: Duration) -> Scheduler {
        Scheduler { debounce, running: false, pending: false, deadline: None, last_event: None }
    }

    /// The check that is about to start was requested outside the debounce
    /// loop (the first check, when the command starts).
    pub fn begin(&mut self) {
        self.running = true;
        self.pending = false;
        self.deadline = None;
    }

    pub fn running(&self) -> bool {
        self.running
    }

    pub fn is_pending(&self) -> bool {
        self.pending
    }

    /// A relevant path changed and the save is visible.
    pub fn on_relevant(&mut self, now: Instant) {
        self.last_event = Some(now);
        if self.running {
            self.pending = true;
        } else {
            self.deadline = Some(now + self.debounce);
        }
    }

    /// The files on disk disagree with the snapshot taken when the running
    /// check started, even if no event has been delivered yet.
    pub fn note_dirty(&mut self) {
        if self.running {
            self.pending = true;
        }
    }

    /// How long until the idle debounce fires. None while a check runs or
    /// nothing is waiting.
    pub fn time_until(&self, now: Instant) -> Option<Duration> {
        if self.running {
            return None;
        }
        self.deadline.map(|d| d.saturating_duration_since(now))
    }

    /// Start the debounced check when its deadline has arrived. The returned
    /// instant is the edit this check is answering (the last one that armed
    /// or reset the deadline). None when no check should start.
    pub fn poll(&mut self, now: Instant) -> Option<Instant> {
        if self.running {
            return None;
        }
        let deadline = self.deadline?;
        if now < deadline {
            return None;
        }
        let trigger = self.last_event;
        self.deadline = None;
        self.running = true;
        self.pending = false;
        trigger
    }

    /// The running check has finished. Returns whether a relevant change
    /// landed during it (the diagnostics are stale). When that happened,
    /// exactly one follow-up is armed.
    pub fn on_finished(&mut self, now: Instant) -> bool {
        let stale = self.pending;
        self.running = false;
        self.pending = false;
        if stale {
            self.deadline = Some(now + self.debounce);
        }
        stale
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(base: Instant, ms: u64) -> Instant {
        base + Duration::from_millis(ms)
    }

    #[test]
    fn rapid_edits_become_one_check() {
        let base = Instant::now();
        let mut s = Scheduler::new(Duration::from_millis(100));
        for i in 0..8 {
            s.on_relevant(at(base, i * 10));
            assert!(s.poll(at(base, i * 10 + 5)).is_none());
        }
        // The deadline is the last edit plus the debounce, not the first.
        assert!(s.poll(at(base, 169)).is_none());
        assert!(s.poll(at(base, 170)).is_some());
        assert!(s.running());
        assert!(s.poll(at(base, 500)).is_none(), "a running check does not start another");
    }

    #[test]
    fn edits_during_a_check_schedule_exactly_one_follow_up() {
        let base = Instant::now();
        let mut s = Scheduler::new(Duration::from_millis(50));
        s.on_relevant(base);
        assert!(s.poll(at(base, 50)).is_some());
        s.on_relevant(at(base, 60));
        s.on_relevant(at(base, 70));
        s.on_relevant(at(base, 80));
        assert!(s.poll(at(base, 90)).is_none());
        assert!(s.on_finished(at(base, 100)));
        assert!(!s.running());
        // Not a second or a third check: one deadline.
        assert!(s.poll(at(base, 149)).is_none());
        assert!(s.poll(at(base, 150)).is_some());
        // An edit during the follow-up waits for that check to finish.
        s.on_relevant(at(base, 160));
        assert!(s.poll(at(base, 400)).is_none());
        assert!(s.on_finished(at(base, 410)));
        assert!(s.poll(at(base, 459)).is_none());
        assert!(s.poll(at(base, 460)).is_some());
    }

    #[test]
    fn trailing_edits_reset_the_single_follow_up() {
        let base = Instant::now();
        let mut s = Scheduler::new(Duration::from_millis(50));
        s.begin();
        s.on_relevant(at(base, 10));
        assert!(s.on_finished(at(base, 20)));
        s.on_relevant(at(base, 40));
        assert!(s.poll(at(base, 70)).is_none());
        assert!(s.poll(at(base, 90)).is_some());
        assert!(!s.on_finished(at(base, 100)));
        assert!(s.poll(at(base, 200)).is_none());
    }

    #[test]
    fn quiet_finish_does_not_schedule() {
        let base = Instant::now();
        let mut s = Scheduler::new(Duration::from_millis(50));
        s.begin();
        assert!(!s.on_finished(base));
        assert!(s.poll(at(base, 1000)).is_none());
    }
}
