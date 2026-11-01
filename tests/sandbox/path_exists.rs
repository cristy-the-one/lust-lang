// `Path::exists` reaches the file system through std's own code: check
// cannot see it, so the built program stops before the call.
fn main() {
    println!("before");
    println!("{}", std::path::Path::new("/").exists());
}
