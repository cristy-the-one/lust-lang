#[derive(Debug)]
pub struct Refusal;

pub struct Session {
    pub n: u32,
}

impl Session {
    #[track_caller]
    pub fn apply(&mut self, op: &str) -> Result<u32, Refusal> {
        self.n += 1;
        if op.is_empty() { Err(Refusal) } else { Ok(self.n) }
    }
    pub fn twice(&mut self, op: &str) -> Result<u32, Refusal> {
        self.apply(op)?; // the crate's own call: unread, in its own file
        self.apply(op)
    }
}
