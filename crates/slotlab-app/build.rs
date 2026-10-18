//! Compiles `csrc/zstd_resume.c`, the few lines of C that let SlotLab resume
//! a zstd frame in the middle (`src/books/zran.rs`).
//!
//! The shim reads the decoder's internal state, so it MUST be compiled
//! against the very sources of the libzstd it is linked with: zstd-sys
//! exports their directory (`links = "zstd"` → `DEP_ZSTD_INCLUDE`), which is
//! why slotlab-app depends on zstd-sys directly.

fn main() {
    println!("cargo:rerun-if-changed=csrc/zstd_resume.c");
    let include = std::env::var("DEP_ZSTD_INCLUDE")
        .expect("DEP_ZSTD_INCLUDE: zstd-sys must be built from its bundled sources (not pkg-config)");
    let lib = std::path::PathBuf::from(include.split(';').next().unwrap_or_default());
    assert!(
        lib.join("decompress/zstd_decompress_internal.h").is_file(),
        "zstd internal headers not found under {} — the zstd resume shim needs zstd-sys's bundled sources",
        lib.display()
    );
    cc::Build::new()
        .file("csrc/zstd_resume.c")
        .include(&lib)
        .include(lib.join("common"))
        .include(lib.join("decompress"))
        .opt_level(3)
        .warnings(true)
        .compile("slotlab_zstd_resume");
}
