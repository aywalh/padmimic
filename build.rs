fn main() {
    println!("cargo:rerun-if-changed=assets/branding/padmimic.res");
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        let resource = std::path::Path::new(&std::env::var_os("CARGO_MANIFEST_DIR").unwrap())
            .join("assets/branding/padmimic.res");
        println!("cargo:rustc-link-arg-bin=padmimic={}", resource.display());
    }
}
