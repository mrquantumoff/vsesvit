//! Owned by W0.1.
//! Linux: the examples find the libraries `xtask probe` stages beside them, as the binaries do
//! (`-Wl,-rpath,$ORIGIN`, design section 6.1).

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("linux") {
        println!("cargo::rustc-link-arg-examples=-Wl,-rpath,$ORIGIN");
    }
}
