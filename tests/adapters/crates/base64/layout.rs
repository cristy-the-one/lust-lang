// lust-reject: L0652
// The adapter engine is a pair of flags, not upstream's tables, so lust
// must not answer size_of for it.
fn main() {
    println!("{}", std::mem::size_of::<base64::engine::general_purpose::GeneralPurpose>() > 0);
}
