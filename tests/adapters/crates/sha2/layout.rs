// lust-reject: L0652
// The adapter's Sha256 is a state buffer, not upstream's wrapper, so
// lust must not answer size_of for it.
fn main() {
    println!("{}", std::mem::size_of::<sha2::Sha256>() > 0);
}
