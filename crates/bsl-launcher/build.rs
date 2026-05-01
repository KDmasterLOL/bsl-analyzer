fn main() {
    // `cfg(target_os = "windows")` in a build script evaluates against the
    // build *host*, so under `cargo xwin` cross-compilation from Linux it
    // would always be false and the VersionInfo would silently be missing
    // from the artifact. Use `CARGO_CFG_TARGET_OS` to inspect the actual
    // target instead.
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
    // Fail loudly so a CI image without the resource compiler produces
    // a visibly broken pipeline rather than a silently shippable
    // binary that's missing its VersionInfo.
    //
    // On Windows host: needs MSVC's `rc.exe` (bundled with VS).
    // On Linux host (cargo-xwin cross-compile): winresource looks for
    // `llvm-rc` in PATH — install Debian's `llvm` package (which
    // provides `/usr/bin/llvm-rc-XX` plus a `llvm-rc` symlink) into
    // the CI image.
    res.compile().expect("failed to embed Windows resources for bsl-launcher");
}
