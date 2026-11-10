// `--mark-unread Session::apply`: each call sits on its own line, and the
// comment at its end says whether its result is read.
struct Session {
    n: u32,
}

#[derive(Debug)]
struct Refusal;

impl Session {
    #[track_caller]
    fn apply(&mut self, op: &str) -> Result<u32, Refusal> {
        self.n += 1;
        if op.is_empty() { Err(Refusal) } else { Ok(self.n) }
    }
    fn other(&mut self, _op: &str) -> Result<u32, Refusal> {
        Ok(0)
    }
}

fn apply(s: &mut Session) -> Result<u32, Refusal> {
    s.other("free fn of the same name") // not Session::apply
}

fn helper(s: &mut Session) -> Result<u32, Refusal> {
    s.apply("helper") // read: the tail value
}

fn run(s: &mut Session) -> Result<(), Refusal> {
    s.apply("a"); // unread
    s.apply("b")?; // unread
    s.apply("c").unwrap(); // unread
    s.apply("d").expect("d"); // unread
    let _ = s.apply("e"); // unread
    let _ = s.apply("f")?; // unread
    if s.apply("g").is_ok() {} // unread
    let bad = s.apply("").is_err(); // unread
    match s.apply("h") { // unread
        Ok(_) => {}
        Err(e) => println!("{e:?}"),
    }
    match s.apply("i") { // unread
        Ok(_) if s.n > 100 => {}
        _ => {}
    }
    if let Ok(_) = s.apply("j") {} // unread
    if let Err(_) = s.apply("k") {} // unread
    s
        .apply("l")?; // unread, reported where `apply` is written
    Session::apply(s, "m")?; // unread, a path call
    // s.apply("in a comment");
    let text = "s.apply(\"in a string\");";

    let r = s.apply("n")?; // read
    let _unused = s.apply("o")?; // read (a binding, for now)
    println!("{}", s.apply("p")?); // read
    match s.apply("q") { // read
        Ok(v) => println!("{v}"),
        Err(_) => {}
    }
    if let Ok(v) = s.apply("r") { // read
        println!("{v}");
    }
    let v = s.apply("s").unwrap_or(0); // read
    let w = [s.apply("t").unwrap()]; // read
    s.other("u")?; // another fn
    apply(s)?; // the free fn
    helper(s)?; // the helper's own call is read
    println!("{bad} {r} {v} {w:?} {}", text.len());
    Ok(())
}

fn main() {
    let mut s = Session { n: 0 };
    run(&mut s).unwrap();
    println!("{}", s.n);
}
