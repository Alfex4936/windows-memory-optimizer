fn main() {
    println!("cargo:rerun-if-changed=assets/memori.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        winresource::WindowsResource::new()
            .set_icon("assets/memori.ico")
            .set("ProductName", "Memori")
            .set(
                "FileDescription",
                "Memori - Windows memory and startup manager",
            )
            .set("InternalName", "memori")
            .set("OriginalFilename", "memori.exe")
            .set(
                "LegalCopyright",
                "Copyright (c) 2026 Alfex4936 and Memori contributors",
            )
            .compile()
            .expect("Windows icon and version resources could not be compiled");
    }
}
