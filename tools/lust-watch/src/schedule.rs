//! One check at a time. The deadline is one debounce after the saved bytes
//! last changed.
//!
//! A file's mtime does not always move on a rapid overwrite (APFS can keep
//! the previous timestamp across a short burst), so the quiet window is the
//! bytes, not the timestamp. Each new fingerprint starts that window over.
//! The same fingerprint again (a second FSEvents batch for bytes already
//! seen) does not. A save while a check runs sets one pending bit. The
//! finish does not arm a follow-up; the caller arms exactly one.

use std::time::{Duration, Instant};

#[derive(Debug)]
pub struct Scheduler {
    debounce: Duration,
    running: bool,
    pending: bool,
    deadline: Option<Instant>,
    last_event: Option<Instant>,
    /// Bytes the current quiet window is waiting on.
    armed: Option<u64>,
}

impl Scheduler {
    pub fn new(debounce: Duration) -> Scheduler {
        Scheduler { debounce, running: false, pending: false, deadline: None, last_event: None, armed: None }
    }

    /// The check that is about to start was requested outside the debounce
    /// loop (the first check, when the command starts).
    pub fn begin(&mut self) {
        self.running = true;
        self.pending = false;
        self.deadline = None;
        self.armed = None;
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

    /// `fp` is the saved tree. A new fingerprint starts the quiet window
    /// over (`now + debounce`). The same fingerprint leaves the window
    /// where it is, so a second kernel batch cannot delay it or split it
    /// into two checks. While a check runs, this only records that one
    /// follow-up is owed.
    pub fn on_save(&mut self, now: Instant, fp: u64) {
        if self.running {
            self.pending = true;
            return;
        }
        if self.armed == Some(fp) && self.deadline.is_some() {
            return;
        }
        self.armed = Some(fp);
        self.last_event = Some(now);
        self.deadline = Some(now + self.debounce);
    }

    pub fn armed(&self) -> Option<u64> {
        self.armed
    }

    /// The saved bytes match the check already finished. A late kernel event
    /// for that same tree must not start another check.
    pub fn on_unchanged(&mut self) {
        if !self.running {
            self.deadline = None;
            self.last_event = None;
            self.armed = None;
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
        self.armed = None;
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
        self.armed = None;
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
            // Each new fingerprint restarts the one quiet window.
            let saved_at = i * 10;
            s.on_save(at(base, saved_at), i);
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
        s.on_save(base, 1);
        assert!(s.poll(at(base, 50)).is_some());
        s.on_save(at(base, 60), 2);
        s.on_save(at(base, 70), 3);
        s.on_save(at(base, 80), 4);
        assert!(s.poll(at(base, 200)).is_none());
        assert!(s.on_finished());
        assert!(!s.running());
        // Finish does not invent a deadline. The caller arms one follow-up.
        assert!(s.poll(at(base, 500)).is_none());
        s.on_save(at(base, 100), 5);
        assert!(s.poll(at(base, 149)).is_none());
        assert!(s.poll(at(base, 150)).is_some());
        // An edit during the follow-up waits for that check to finish,
        // and again becomes exactly one arm.
        s.on_save(at(base, 160), 6);
        assert!(s.poll(at(base, 400)).is_none());
        assert!(s.on_finished());
        assert!(s.poll(at(base, 500)).is_none());
        s.on_save(at(base, 410), 7);
        assert!(s.poll(at(base, 459)).is_none());
        assert!(s.poll(at(base, 460)).is_some());
    }

    #[test]
    fn trailing_edits_reset_the_single_follow_up() {
        let base = Instant::now();
        let mut s = Scheduler::new(Duration::from_millis(50));
        s.begin();
        s.on_save(at(base, 10), 1);
        assert!(s.on_finished());
        s.on_save(at(base, 20), 2);
        s.on_save(at(base, 40), 3);
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
        s.on_save(base, 1);
        s.on_unchanged();
        assert!(s.poll(at(base, 120)).is_none());
        assert!(s.poll(at(base, 500)).is_none());
    }

    #[test]
    fn a_new_fingerprint_restarts_the_one_window() {
        let base = Instant::now();
        let mut s = Scheduler::new(Duration::from_millis(100));
        s.on_save(base, 1);
        // Same bytes reported again do not move the deadline.
        s.on_save(at(base, 40), 1);
        assert!(s.poll(at(base, 100)).is_some());
        assert!(!s.on_finished());
        // A different save seen at the old deadline starts one new window,
        // instead of a check of the bytes that have already moved on.
        s.on_save(at(base, 100), 1);
        s.on_save(at(base, 100), 2);
        assert!(s.poll(at(base, 100)).is_none());
        assert!(s.poll(at(base, 199)).is_none());
        assert!(s.poll(at(base, 200)).is_some());
    }
}
