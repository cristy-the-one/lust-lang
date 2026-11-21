// expect: L0113
fn main() {
    let parts: Vec<&str> = "a/./b".split('/').filter(|c| !c.is_empty() && c != ".").collect();
    println!("{}", parts.join("/"));
}
