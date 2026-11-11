// `--time-limit`: the mode is a line on stdin (a sandboxed program reads
// no arguments). `spin` loops, `sleep` waits far longer than a short
// limit, `finish` returns at once.
use std::io::Read;

fn main() {
    println!("before");
    let mut mode = String::new();
    std::io::stdin().read_to_string(&mut mode).unwrap();
    match mode.trim() {
        "spin" => loop {},
        "sleep" => {
            std::thread::sleep(std::time::Duration::from_secs(30));
            println!("after");
        }
        "finish" => println!("done"),
        other => println!("unknown {other}"),
    }
}
