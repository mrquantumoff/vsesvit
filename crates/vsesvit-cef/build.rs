//! Owned by W1.3.
//! Linux: the examples sit in `target/<profile>/examples` and the unit-test binary in
//! `target/<profile>/deps`, one level below the `libcef.so` cef-dll-sys copies, so both get an
//! `$ORIGIN/..` rpath; the shipped bins set their own (design sections 1.2 and 3.5). Windows: nothing;
//! cef-dll-sys copies the runtime beside the bins, and tests need that folder on PATH.

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("linux") {
        println!("cargo::rustc-link-arg=-Wl,-rpath,$ORIGIN/..");
    }
}
