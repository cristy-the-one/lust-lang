// expect: L0610
// rustc E0282: `last.1` cannot fix the element type of `merged`.
fn main() {
    let mut merged = Vec::new();
    if let Some(last) = merged.last_mut() {
        last.1 = 2;
    }
    merged.push((1usize, 2usize));
    println!("{}", merged.len());
}
