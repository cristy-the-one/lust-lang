//! `lust watch check` scheduling and watch-set rules.
//!
//! This crate is a separate package from `lust` and `lust-core`. The client
//! execs `lust-watch` only for the `watch` command, so a normal check, build,
//! or run does not load or start the file watcher.

mod files;
mod parse;
mod schedule;

pub use files::{fingerprint, is_real_dir, is_relevant, is_skipped_dir, project_root, survey, watch_plan, Plan, Survey};
pub use parse::{parse_watch, usage, CheckCmd};
pub use schedule::Scheduler;
