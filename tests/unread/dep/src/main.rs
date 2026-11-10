use kit::Session;

fn main() {
    let mut s = Session { n: 0 };
    s.apply("a").unwrap(); // unread
    let n = s.apply("b").unwrap(); // read
    s.twice("c").unwrap(); // another fn
    println!("{n} {}", s.n);
}
