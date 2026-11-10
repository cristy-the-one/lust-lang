// The type lives in a module and is named from the crate root, as a host
// names it: `kit::Session::apply`.
mod session;
pub use session::{Refusal, Session};
