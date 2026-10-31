// lust-reject: L0652
fn main() {
    println!("{}", std::mem::size_of::<libm::Libm<f64>>());
}
