// A comma inside a turbofish does not end a macro argument: rustc's
// `$x:expr` reads the whole path. In `matches!`, `println!`, `format!`,
// `assert_eq!`, `assert!`, `write!` and `vec!`, with nested generics (a
// `>>` that closes two), and beside `<` and `>` that are comparisons.
use std::collections::BTreeMap;
use std::fmt::Write;

fn pair<A, B>(a: A, b: B) -> (A, B) {
    (a, b)
}

fn first<A: Clone, B>(p: &(A, B)) -> A {
    p.0.clone()
}

#[derive(Debug, PartialEq)]
struct Loud(u8);

fn main() {
    println!("{}", matches!(Err::<u8, Loud>(Loud(1)), Err(Loud(1))));
    println!("{}", matches!(Ok::<u8, Loud>(2), Ok(n) if n > 1));
    println!("{:?} {:?}", pair::<u8, &str>(1, "a"), pair::<Vec<u8>, Option<u8>>(vec![1], None));
    println!("{}", first::<u8, Vec<Vec<u8>>>(&(3, vec![vec![4]])));
    let s = format!("{:?}|{}", BTreeMap::<u8, u8>::new(), Vec::<(u8, u8)>::new().len());
    println!("{s}");
    assert_eq!(pair::<u8, u8>(1, 2), (1, 2));
    assert_eq!(pair::<u8, u8>(1, 2), pair::<u8, u8>(1, 2), "with a message {}", 1);
    let (a, b) = (1, 2);
    assert!(a < b, "comparisons still split: {} {}", a, b > a);
    println!("{} {}", a < b, b > a);
    let mut out = String::new();
    write!(out, "{:?}", pair::<u8, u8>(5, 6)).unwrap();
    writeln!(out, " {}", first::<u8, u8>(&(7, 8))).unwrap();
    print!("{out}");
    let v = vec![pair::<u8, u8>(1, 2), pair::<u8, u8>(3, 4)];
    println!("{v:?}");
    println!("{}", matches!(pair::<u8, u8>(1, 2), (1, _)));
}
