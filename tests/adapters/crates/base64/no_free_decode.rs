// lust-reject: L0506
// The deprecated free function is not in the adapter. Call the engine.
use base64::decode;

fn main() {
    println!("{:?}", decode(b"Zg=="));
}
