// std::os::unix::process: ExitStatusExt over raw wait statuses (exit, signal,
// core dump, stop, continue, unrecognised) and as a fn value, then
// CommandExt::exec: a failed exec returns its error, a working one replaces the
// process (the unflushed `print!` tail is lost, as with rustc's LineWriter).
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::process::{Command, ExitStatus};

fn main() {
    for raw in [0, 0x300, 9, 0x8b, 0x137f, 0xffff, 0xff, -1] {
        let s = ExitStatus::from_raw(raw);
        println!("{s} | {:?} {:?} {} {:?} {} {}", s.code(), s.signal(), s.core_dumped(), s.stopped_signal(), s.continued(), s.into_raw());
    }
    let st = Command::new("sh").args(["-c", "kill -9 $$"]).status().unwrap();
    println!("{:?}", Some(st).as_ref().and_then(ExitStatusExt::signal));
    let e = Command::new("/no/such/program").exec();
    println!("{:?} {e}", e.kind());
    println!("{:?}", Command::new("echo").arg("a\0b").exec());
    print!("lost tail");
    let e = Command::new("sh").args(["-c", "echo exec $X; pwd"]).env("X", "ok").current_dir("/").exec();
    println!("unreachable {e}");
}
