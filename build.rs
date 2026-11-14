fn main() {
    // Assets are embedded into the exe; rebuild whenever any of them changes.
    println!("cargo:rerun-if-changed=assets");
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon_with_id("assets/icon.ico", "1");
        res.set("ProductName", "Inkdown");
        res.set("FileDescription", "Inkdown Markdown Editor");
        res.set("CompanyName", "Inkdown");
        res.set("LegalCopyright", "");
        res.compile().expect("failed to compile Windows resources");
    }
}
