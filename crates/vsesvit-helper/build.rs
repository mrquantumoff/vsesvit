//! Owned by W1.3.
//! Linux: `$ORIGIN` rpath, so the helper finds `libcef.so` beside it (design section 3.5, G14).

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("linux") {
        println!("cargo::rustc-link-arg-bins=-Wl,-rpath,$ORIGIN");
    }
}
