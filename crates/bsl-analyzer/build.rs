use std::io::Write;
use std::path::Path;

fn main() {
    // `cfg(target_os = "windows")` in a build script evaluates against the
    // build *host*, so under `cargo xwin` cross-compilation from Linux it
    // would always be false and the resources would silently be missing
    // from the artifact. Use `CARGO_CFG_TARGET_OS` to inspect the actual
    // target instead.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set("FileDescription", "BSL Analyzer LSP");
        res.set("ProductName", "BSL Analyzer");
        res.set("CompanyName", "1C BSL Analyzer contributors");
        res.set("LegalCopyright", "Copyright (C) 1C BSL Analyzer contributors");
        res.set("OriginalFilename", "bsl-analyzer-app.exe");
        res.set("InternalName", "bsl-analyzer-app.exe");
        // Fail loudly so a CI image without `windres` / `rc.exe` produces a
        // visibly broken pipeline rather than a silently shippable binary
        // that's missing its VersionInfo. To unblock cross-compile in CI,
        // install `mingw-w64` (provides `windres`) into the build image.
        res.compile().expect("failed to embed Windows resources for bsl-analyzer");
    }

    let extension_dir = Path::new("../../extension/src");
    let out_dir = std::env::var("OUT_DIR").unwrap();
    let zip_path = Path::new(&out_dir).join("extension.zip");

    let file = std::fs::File::create(&zip_path).expect("failed to create extension.zip");
    let mut zip = zip::ZipWriter::new(file);

    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);

    for entry in walkdir::WalkDir::new(extension_dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
    {
        let path = entry.path();
        let rel = path.strip_prefix(extension_dir).unwrap();
        let rel_str = rel.to_string_lossy().replace('\\', "/");

        zip.start_file(&rel_str, options).expect("failed to start zip entry");
        let content = std::fs::read(path).expect("failed to read extension file");
        zip.write_all(&content).expect("failed to write zip entry");

        // Re-run build if any extension file changes.
        println!("cargo:rerun-if-changed={}", path.display());
    }

    zip.finish().expect("failed to finalize zip");
}
