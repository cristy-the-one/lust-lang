// expect: L0102
// `Some` has one field: two patterns do not fit it (rustc E0023).
fn main() {
    let o = Some(1u8);
    match o {
        Some(a, b) => println!("{a} {b}"),
        None => {}
    }
}
