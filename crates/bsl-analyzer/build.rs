use std::io::Write;
use std::path::Path;

fn main() {
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
