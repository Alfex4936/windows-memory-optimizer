fn main() {
    println!("cargo:rerun-if-changed=assets/memori.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let icon = std::fs::read("assets/memori.ico").expect("Memori icon could not be read");
        // Use a deterministic FNV-1a fingerprint so only icon changes affect taskbar grouping.
        let revision = icon.iter().fold(0xcbf29ce484222325_u64, |hash, byte| {
            (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
        });
        println!("cargo:rustc-env=MEMORI_ICON_REVISION={revision:016x}");
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
