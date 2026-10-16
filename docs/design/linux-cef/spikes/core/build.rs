fn main() {
    // Let the dynamic loader find libcef.so next to the executable.
    println!("cargo::rustc-link-arg-bins=-Wl,-rpath,$ORIGIN");
}
