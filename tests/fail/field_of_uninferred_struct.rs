// expect: L0610
// rustc E0282: `last.x` cannot fix the element type of `v`.
struct P {
    x: i32,
}

fn main() {
    let mut v = Vec::new();
    if let Some(last) = v.last() {
        println!("{}", last.x);
    }
    v.push(P { x: 1 });
}
