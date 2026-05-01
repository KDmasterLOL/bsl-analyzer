fn main() {
    #[cfg(target_os = "windows")]
    {
        let mut res = winresource::WindowsResource::new();
        res.set("FileDescription", "BSL Analyzer launcher");
        res.set("ProductName", "BSL Analyzer");
        res.set("CompanyName", "1C BSL Analyzer contributors");
        res.set("LegalCopyright", "Copyright (C) 1C BSL Analyzer contributors");
        res.set("OriginalFilename", "bsl-analyzer.exe");
        res.set("InternalName", "bsl-analyzer.exe");
        res.compile().expect("failed to compile windows resources");
    }
}
