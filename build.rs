fn main() {
    if cfg!(target_env = "gnu") {
        println!("cargo:rustc-link-lib=static=z");
        println!("cargo:rustc-link-search=all=/musl/usr/local/lib");
    }
}
