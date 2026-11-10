// `Some(..)`, `Ok(..)` and `Err(..)`: a lone `..` stands for the one field
// without naming it, as `_` does; beside a pattern (`Some(x, ..)`,
// `Ok(.., v)`) it stands for no field. In `match`, `if let`, `while let`,
// `let .. else`, `matches!`, nested, with a guard, and where the value
// behind it has a destructor that must still run once.
struct Loud(&'static str);

impl Drop for Loud {
    fn drop(&mut self) {
        println!("drop {}", self.0);
    }
}

fn kind(r: &Result<u32, String>) -> &'static str {
    match r {
        Ok(..) => "ok",
        Err(..) => "err",
    }
}

fn has(o: Option<Vec<u8>>) -> bool {
    match o {
        Some(..) => true,
        None => false,
    }
}

fn nested(v: Option<Result<u32, String>>) -> u32 {
    match v {
        Some(Ok(..)) => 1,
        Some(Err(..)) => 2,
        None => 3,
    }
}

fn guarded(r: Result<u32, u32>, big: bool) -> &'static str {
    match r {
        Ok(..) if big => "big ok",
        Ok(n) => {
            if n > 1 { "ok many" } else { "ok" }
        }
        Err(..) => "err",
    }
}

fn first_ok(items: &[Result<u32, u32>]) -> usize {
    let mut it = items.iter();
    let mut n = 0;
    while let Some(Err(..)) = it.next() {
        n += 1;
    }
    n
}

fn must(r: Result<Loud, Loud>) -> &'static str {
    let Ok(..) = r else {
        return "refused";
    };
    "taken"
}

// A `..` beside a pattern stands for no field: the pattern is the field's.
fn beside(o: Option<u32>, r: Result<Loud, Loud>) -> u32 {
    let n = match o {
        Some(x, ..) => x,
        None => 0,
    };
    let m = if let Some(.., y) = o { y } else { 0 };
    match r {
        Ok(.., v) => println!("ok {}", v.0),
        Err(e, ..) => println!("err {}", e.0),
    }
    n + m
}

fn main() {
    println!("{}", beside(Some(4), Ok(Loud("beside ok"))));
    println!("{}", beside(None, Err(Loud("beside err"))));
    println!("{} {}", kind(&Ok(1)), kind(&Err("x".to_string())));
    println!("{} {}", has(Some(vec![1, 2])), has(None));
    println!("{} {} {}", nested(Some(Ok(1))), nested(Some(Err("e".into()))), nested(None));
    println!("{} {} {}", guarded(Ok(5), true), guarded(Ok(5), false), guarded(Err(0), true));
    println!("{}", first_ok(&[Err(1), Err(2), Ok(3), Err(4)]));
    if let Some(..) = Some(Loud("in if let")) {
        println!("some");
    }
    if let Err(..) = Ok::<u8, u8>(1) {
        println!("unreachable");
    }
    println!("{}", must(Ok(Loud("ok value"))));
    println!("{}", must(Err(Loud("err value"))));
    let o = Some(Loud("kept"));
    println!("{}", matches!(o, Some(..)));
    let e: Result<u8, Loud> = Err(Loud("temp"));
    println!("{}", matches!(e, Ok(..)));
    println!("end");
}
