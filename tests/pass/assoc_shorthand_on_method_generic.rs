// `S::Ok` where `S` is a type parameter of the method itself, in a trait
// method's signature: the method's own bounds name associated types there
// as they do in an impl method and in a free fn. This is the shape of
// serde's `Serialize::serialize`.
use std::fmt::Display;

trait Sink: Sized {
    type Ok;
    type Error: Display;
    fn put(self, v: u32) -> Result<Self::Ok, Self::Error>;
}

trait Emit {
    // Inline bound, two projections.
    fn emit<S: Sink>(&self, s: S) -> Result<S::Ok, S::Error>;
    // A `where` bound, and a default body that uses the projection.
    fn emit_or<S>(&self, s: S, fallback: S::Ok) -> S::Ok
    where
        S: Sink,
    {
        match self.emit(s) {
            Ok(v) => v,
            Err(_) => fallback,
        }
    }
    // The projection in a parameter position only.
    fn describe<S: Sink>(&self, r: &Result<S::Ok, S::Error>) -> String
    where
        S::Ok: Display,
    {
        match r {
            Ok(v) => format!("ok {v}"),
            Err(e) => format!("err {e}"),
        }
    }
}

struct P(u32);

impl Emit for P {
    fn emit<S: Sink>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.put(self.0)
    }
}

struct Text;
impl Sink for Text {
    type Ok = String;
    type Error = String;
    fn put(self, v: u32) -> Result<String, String> {
        if v == 0 { Err("zero".to_string()) } else { Ok(format!("<{v}>")) }
    }
}

struct Count<'a>(&'a mut u32);
impl<'a> Sink for Count<'a> {
    type Ok = u32;
    type Error = &'static str;
    fn put(self, v: u32) -> Result<u32, &'static str> {
        *self.0 += v;
        Ok(*self.0)
    }
}

fn free<S: Sink>(s: S) -> Result<S::Ok, S::Error> {
    s.put(1)
}

fn main() {
    println!("{:?} {:?}", P(7).emit(Text), P(0).emit(Text));
    println!("{} {}", P(7).emit_or(Text, "none".to_string()), P(0).emit_or(Text, "none".to_string()));
    let mut n = 10;
    println!("{:?}", P(5).emit(Count(&mut n)));
    println!("{}", P(5).emit_or(Count(&mut n), 0));
    println!("{}", P(1).describe::<Text>(&P(3).emit(Text)));
    println!("{}", P(1).describe::<Text>(&P(0).emit(Text)));
    println!("{:?} {n}", free(Text));
}
