fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    let mut res = winresource::WindowsResource::new();
    res.set("FileDescription", "BSL Analyzer launcher");
    res.set("ProductName", "BSL Analyzer");
    res.set("CompanyName", "1C BSL Analyzer contributors");
    res.set("LegalCopyright", "Copyright (C) 1C BSL Analyzer contributors");
    res.set("OriginalFilename", "bsl-analyzer.exe");
    res.set("InternalName", "bsl-analyzer.exe");
    res.compile().expect("failed to embed Windows resources for bsl-launcher");
}
