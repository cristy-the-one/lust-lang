#[cfg(unix)]
fn one() -> i64 {
    1
}

fn main() {
    println!("{}", one());
}
