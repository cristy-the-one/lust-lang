// Array, tuple and struct operands are evaluated left to right, and a place operand keeps the
// value it had where it was written when a later operand writes that place (lower.rs
// `ordered_operands`). The effect sits at the end, in the middle and at the start of the list,
// and a list with no effect after a place reads the place in place. The long table has no
// effects: its lowering must not ask every element about every later one.
#[derive(Debug)]
struct S {
    a: u32,
    b: u32,
    c: u32,
}

fn bump(x: &mut u32) -> u32 {
    *x += 10;
    *x
}

static TABLE: [u16; 64] = [
    0, 1, 2, 3, 3, 3, 4, 4, 4, 4, 4, 5, 5, 5, 5, 5, 5, 6, 6, 7, 7, 7, 7, 9, 9, 10, 11, 11, 11, 12, 12, 12, 12, 13, 13, 14, 15, 15, 16, 16, 17,
    18, 18, 19, 19, 19, 20, 21, 21, 22, 23, 23, 24, 25, 25, 26, 27, 27, 28, 29, 29, 30, 31, 31,
];

fn main() {
    let mut x = 1u32;
    let last = [x, x, { x += 1; x }];
    println!("{last:?} {x}");

    let mut x = 1u32;
    let middle = [x, { x += 1; x }, x, x];
    println!("{middle:?} {x}");

    let mut x = 1u32;
    let first = [{ x += 1; x }, x, x];
    println!("{first:?} {x}");

    let mut x = 1u32;
    let two = [x, { x += 1; x }, x, { x += 1; x }, x];
    println!("{two:?} {x}");

    let mut x = 1u32;
    let call = [x, bump(&mut x), x, bump(&mut x)];
    println!("{call:?} {x}");

    let mut x = 1u32;
    let tuple = (x, { x += 1; x }, x);
    println!("{tuple:?} {x}");

    let mut x = 1u32;
    let s = S { a: x, b: { x += 1; x }, c: x };
    println!("{s:?} {x}");

    let x = 7u32;
    let plain = [x, x, 3, x];
    println!("{plain:?}");

    let mut v = vec![1u32, 2, 3];
    let lens = [v.len(), { v.push(4); v.len() }, v.len()];
    println!("{lens:?}");

    let sum: u32 = TABLE.iter().map(|&n| n as u32).sum();
    println!("{} {} {sum}", TABLE.len(), TABLE[63]);
}
