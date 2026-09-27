fn main() {
    // Read with `option_env!` by `Config::builtin`.
    println!("cargo::rerun-if-env-changed=VSESVIT_UPDATER_PUBKEY");
    println!("cargo::rerun-if-env-changed=VSESVIT_UPDATER_ENDPOINTS");
}
