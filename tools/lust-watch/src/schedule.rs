//! One check at a time. The deadline is the quiet time of the newest save,
//! not the moment a kernel event showed up.
//!
//! macOS FSEvents (latency 0, `kFSEventStreamEventIdSinceNow`) still delivers
//! a save late, and can deliver the same save again in a later batch, including
//! a notification for the tree that existed when the stream started. A second
//! batch is not a second check: the caller drops the deadline when the saved
//! bytes match the check already run, and moves this one deadline when a newer
//! save is still inside the quiet period. A save that arrives while a check is
//! running sets one pending bit. The finish does not arm a follow-up by itself;
//! the caller arms exactly one, from the tree's mtime.

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

    pub fn debounce(&self) -> Duration {
        self.debounce
    }

    pub fn is_pending(&self) -> bool {
        self.pending
    }

    /// The saved tree differs from the last settled check. `quiet_at` is when
    /// the newest save is old enough to check (the save's mtime plus the
    /// debounce, which may already be `now` when the kernel delivered the
    /// event late). A later save replaces that one deadline. While a check
    /// runs, this only records that one follow-up is owed.
    pub fn on_save(&mut self, now: Instant, quiet_at: Instant) {
        if self.running {
            self.pending = true;
            return;
        }
        self.last_event = Some(now);
        self.deadline = Some(quiet_at);
    }

    /// The saved bytes match the check already finished. A late kernel event
    /// for that same tree must not start another check.
    pub fn on_unchanged(&mut self) {
        if !self.running {
            self.deadline = None;
            self.last_event = None;
        }
    }

    /// The quiet time moved (a save landed after the deadline was chosen)
    /// but the edit this check answers stays the one already recorded.
    pub fn postpone(&mut self, quiet_at: Instant) {
        if !self.running && self.deadline.is_some() {
            self.deadline = Some(quiet_at);
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
    /// landed during it. The deadline is cleared either way: a follow-up is
    /// one `on_save` from the caller, using the tree's mtime, so a kernel
    /// batch that shows up later cannot add a second quiet window on top.
    pub fn on_finished(&mut self) -> bool {
        let stale = self.pending;
        self.running = false;
        self.pending = false;
        self.deadline = None;
        self.last_event = None;
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
            // Each save replaces the one quiet deadline. The first edit's
            // deadline (100) must not start a check once a later save exists.
            let saved_at = i * 10;
            s.on_save(at(base, saved_at), at(base, saved_at + 100));
            assert!(s.poll(at(base, saved_at + 5)).is_none());
        }
        assert!(s.poll(at(base, 169)).is_none());
        assert!(s.poll(at(base, 170)).is_some());
        assert!(s.running());
        assert!(s.poll(at(base, 500)).is_none(), "a running check does not start another");
    }

    #[test]
    fn edits_during_a_check_schedule_exactly_one_follow_up() {
        let base = Instant::now();
        let mut s = Scheduler::new(Duration::from_millis(50));
        s.on_save(base, at(base, 50));
        assert!(s.poll(at(base, 50)).is_some());
        s.on_save(at(base, 60), at(base, 110));
        s.on_save(at(base, 70), at(base, 120));
        s.on_save(at(base, 80), at(base, 130));
        assert!(s.poll(at(base, 200)).is_none());
        assert!(s.on_finished());
        assert!(!s.running());
        // Finish does not invent a deadline. The caller arms one follow-up.
        assert!(s.poll(at(base, 500)).is_none());
        s.on_save(at(base, 100), at(base, 150));
        assert!(s.poll(at(base, 149)).is_none());
        assert!(s.poll(at(base, 150)).is_some());
        // An edit during the follow-up waits for that check to finish,
        // and again becomes exactly one arm.
        s.on_save(at(base, 160), at(base, 210));
        assert!(s.poll(at(base, 400)).is_none());
        assert!(s.on_finished());
        assert!(s.poll(at(base, 500)).is_none());
        s.on_save(at(base, 410), at(base, 460));
        assert!(s.poll(at(base, 459)).is_none());
        assert!(s.poll(at(base, 460)).is_some());
    }

    #[test]
    fn trailing_edits_reset_the_single_follow_up() {
        let base = Instant::now();
        let mut s = Scheduler::new(Duration::from_millis(50));
        s.begin();
        s.on_save(at(base, 10), at(base, 60));
        assert!(s.on_finished());
        s.on_save(at(base, 20), at(base, 70));
        s.on_save(at(base, 40), at(base, 90));
        assert!(s.poll(at(base, 70)).is_none());
        assert!(s.poll(at(base, 90)).is_some());
        assert!(!s.on_finished());
        assert!(s.poll(at(base, 200)).is_none());
    }

    #[test]
    fn quiet_finish_does_not_schedule() {
        let base = Instant::now();
        let mut s = Scheduler::new(Duration::from_millis(50));
        s.begin();
        assert!(!s.on_finished());
        assert!(s.poll(at(base, 1000)).is_none());
    }

    #[test]
    fn unchanged_tree_cancels_a_late_event() {
        let base = Instant::now();
        let mut s = Scheduler::new(Duration::from_millis(120));
        // A scan event, or a second FSEvents batch for the bytes just checked.
        s.on_save(base, at(base, 120));
        s.on_unchanged();
        assert!(s.poll(at(base, 120)).is_none());
        assert!(s.poll(at(base, 500)).is_none());
    }

    #[test]
    fn postpone_keeps_a_burst_that_the_kernel_split() {
        let base = Instant::now();
        let mut s = Scheduler::new(Duration::from_millis(100));
        s.on_save(base, at(base, 100));
        // At the first quiet time the newest save is newer than the one
        // that armed the deadline (the kernel had not delivered it yet).
        // The same check waits; it does not start and then start again.
        s.postpone(at(base, 180));
        assert!(s.poll(at(base, 100)).is_none());
        assert!(s.poll(at(base, 179)).is_none());
        assert!(s.poll(at(base, 180)).is_some());
        s.on_save(at(base, 190), at(base, 290));
        assert!(s.is_pending());
        assert!(s.poll(at(base, 400)).is_none());
    }
}
