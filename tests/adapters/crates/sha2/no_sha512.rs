// lust-reject: L0506
// Only Sha256 is in the adapter. Digest is in scope, as a real call needs.
use sha2::{Digest, Sha512};

fn main() {
    let _ = Sha512::new();
}
