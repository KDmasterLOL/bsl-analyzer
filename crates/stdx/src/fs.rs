//! Parallel file system utilities.
//!
//! Provides efficient parallel directory traversal and file reading
//! using the `ignore` crate for maximum performance.

use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use ignore::WalkBuilder;

/// Result of parallel file walking.
pub struct WalkResult<T> {
    items: Vec<T>,
}

impl<T> WalkResult<T> {
    /// Returns the collected items.
    pub fn into_vec(self) -> Vec<T> {
        self.items
    }

    /// Returns the number of items.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Returns true if empty.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

/// Configuration for parallel directory walking.
#[derive(Default)]
pub struct WalkConfig<'a> {
    /// File extensions to include (e.g., ["bsl", "xml"]).
    pub extensions: &'a [&'a str],
    /// Directories to exclude.
    pub excludes: &'a [&'a Path],
    /// Follow symbolic links.
    pub follow_links: bool,
}

/// Count files matching criteria in parallel.
///
/// This is optimized for counting only - doesn't read file contents.
///
/// # Example
///
/// ```ignore
/// use stdx::fs::{parallel_count, WalkConfig};
///
/// let count = parallel_count(
///     &["/path/to/project"],
///     &WalkConfig {
///         extensions: &["bsl", "os"],
///         excludes: &[],
///         follow_links: true,
///     },
/// );
/// ```
pub fn parallel_count(roots: &[&Path], config: &WalkConfig<'_>) -> usize {
    parallel_count_cancellable(roots, config, None)
}

/// Like [`parallel_count`], but observes a shared cancellation flag and
/// returns early once it is set.
///
/// Each `ignore::WalkBuilder` worker checks the flag at the start of every
/// directory entry it visits and returns `WalkState::Quit` to stop further
/// traversal. Already-counted entries are kept; the function returns the
/// partial count so callers can distinguish "no matches" from "aborted".
///
/// Pass `None` to disable cancellation (equivalent to [`parallel_count`]).
pub fn parallel_count_cancellable(
    roots: &[&Path],
    config: &WalkConfig<'_>,
    cancel: Option<&AtomicBool>,
) -> usize {
    let count = AtomicUsize::new(0);

    for root in roots {
        if cancel.is_some_and(|c| c.load(Ordering::Relaxed)) {
            break;
        }

        let mut builder = WalkBuilder::new(root);
        builder.follow_links(config.follow_links).standard_filters(false).hidden(false);

        let excludes = config.excludes;
        let extensions = config.extensions;

        builder.build_parallel().run(|| {
            Box::new(|entry| {
                if cancel.is_some_and(|c| c.load(Ordering::Relaxed)) {
                    return ignore::WalkState::Quit;
                }

                let Ok(entry) = entry else {
                    return ignore::WalkState::Continue;
                };

                let path = entry.path();

                // Check excludes
                if excludes.iter().any(|ex| path.starts_with(ex)) {
                    return ignore::WalkState::Skip;
                }

                // Skip non-files
                let Some(file_type) = entry.file_type() else {
                    return ignore::WalkState::Continue;
                };
                if !file_type.is_file() {
                    return ignore::WalkState::Continue;
                }

                // Check extension
                let ext = path.extension().and_then(|e| e.to_str());
                if ext.is_some_and(|x| extensions.iter().any(|e| e.eq_ignore_ascii_case(x))) {
                    count.fetch_add(1, Ordering::Relaxed);
                }

                ignore::WalkState::Continue
            })
        });
    }

    count.load(Ordering::Relaxed)
}

/// Walk directories in parallel and collect file paths.
///
/// Returns paths to all files matching the criteria.
///
/// # Example
///
/// ```ignore
/// use stdx::fs::{parallel_walk_paths, WalkConfig};
///
/// let paths = parallel_walk_paths(
///     &["/path/to/project"],
///     &WalkConfig {
///         extensions: &["bsl"],
///         ..Default::default()
///     },
/// );
/// ```
pub fn parallel_walk_paths(
    roots: &[&Path],
    config: &WalkConfig<'_>,
) -> WalkResult<std::path::PathBuf> {
    let items = std::sync::Mutex::new(Vec::new());

    for root in roots {
        let mut builder = WalkBuilder::new(root);
        builder.follow_links(config.follow_links).standard_filters(false).hidden(false);

        let excludes = config.excludes;
        let extensions = config.extensions;

        builder.build_parallel().run(|| {
            Box::new(|entry| {
                let Ok(entry) = entry else {
                    return ignore::WalkState::Continue;
                };

                let path = entry.path();

                // Check excludes
                if excludes.iter().any(|ex| path.starts_with(ex)) {
                    return ignore::WalkState::Skip;
                }

                // Skip non-files
                let Some(file_type) = entry.file_type() else {
                    return ignore::WalkState::Continue;
                };
                if !file_type.is_file() {
                    return ignore::WalkState::Continue;
                }

                // Check extension
                let ext = path.extension().and_then(|e| e.to_str());
                if ext.is_some_and(|x| extensions.iter().any(|e| e.eq_ignore_ascii_case(x))) {
                    items.lock().unwrap().push(path.to_path_buf());
                }

                ignore::WalkState::Continue
            })
        });
    }

    WalkResult { items: items.into_inner().unwrap() }
}

/// Walk directories in parallel and read file contents.
///
/// Returns tuples of (path, contents) for all matching files.
/// Files that can't be read are skipped.
///
/// # Example
///
/// ```ignore
/// use stdx::fs::{parallel_read_files, WalkConfig};
///
/// let files = parallel_read_files(
///     &["/path/to/project"],
///     &WalkConfig {
///         extensions: &["xml"],
///         ..Default::default()
///     },
/// );
///
/// for (path, contents) in files.into_vec() {
///     println!("{}: {} bytes", path.display(), contents.len());
/// }
/// ```
pub fn parallel_read_files(
    roots: &[&Path],
    config: &WalkConfig<'_>,
) -> WalkResult<(std::path::PathBuf, Vec<u8>)> {
    let items = std::sync::Mutex::new(Vec::new());

    for root in roots {
        let mut builder = WalkBuilder::new(root);
        builder.follow_links(config.follow_links).standard_filters(false).hidden(false);

        let excludes = config.excludes;
        let extensions = config.extensions;

        builder.build_parallel().run(|| {
            Box::new(|entry| {
                let Ok(entry) = entry else {
                    return ignore::WalkState::Continue;
                };

                let path = entry.path();

                // Check excludes
                if excludes.iter().any(|ex| path.starts_with(ex)) {
                    return ignore::WalkState::Skip;
                }

                // Skip non-files
                let Some(file_type) = entry.file_type() else {
                    return ignore::WalkState::Continue;
                };
                if !file_type.is_file() {
                    return ignore::WalkState::Continue;
                }

                // Check extension
                let ext = path.extension().and_then(|e| e.to_str());
                if ext.is_some_and(|x| extensions.iter().any(|e| e.eq_ignore_ascii_case(x))) {
                    // Read file contents
                    if let Ok(contents) = std::fs::read(path) {
                        items.lock().unwrap().push((path.to_path_buf(), contents));
                    }
                }

                ignore::WalkState::Continue
            })
        });
    }

    WalkResult { items: items.into_inner().unwrap() }
}

/// Walk directories in parallel, read files, and transform with a callback.
///
/// This is the most flexible variant - applies a transformation function
/// to each file's contents in parallel.
///
/// # Example
///
/// ```ignore
/// use stdx::fs::{parallel_read_transform, WalkConfig};
///
/// let parsed = parallel_read_transform(
///     &["/path/to/config"],
///     &WalkConfig {
///         extensions: &["xml"],
///         ..Default::default()
///     },
///     |path, contents| {
///         // Parse XML and return result
///         parse_xml(&contents).ok()
///     },
/// );
/// ```
pub fn parallel_read_transform<T, F>(
    roots: &[&Path],
    config: &WalkConfig<'_>,
    transform: F,
) -> WalkResult<T>
where
    T: Send,
    F: Fn(&Path, Vec<u8>) -> Option<T> + Sync,
{
    let items = std::sync::Mutex::new(Vec::new());

    for root in roots {
        let mut builder = WalkBuilder::new(root);
        builder.follow_links(config.follow_links).standard_filters(false).hidden(false);

        let excludes = config.excludes;
        let extensions = config.extensions;

        builder.build_parallel().run(|| {
            Box::new(|entry| {
                let Ok(entry) = entry else {
                    return ignore::WalkState::Continue;
                };

                let path = entry.path();

                // Check excludes
                if excludes.iter().any(|ex| path.starts_with(ex)) {
                    return ignore::WalkState::Skip;
                }

                // Skip non-files
                let Some(file_type) = entry.file_type() else {
                    return ignore::WalkState::Continue;
                };
                if !file_type.is_file() {
                    return ignore::WalkState::Continue;
                }

                // Check extension
                let ext = path.extension().and_then(|e| e.to_str());
                if ext.is_some_and(|x| extensions.iter().any(|e| e.eq_ignore_ascii_case(x))) {
                    // Read and transform
                    if let Ok(contents) = std::fs::read(path) {
                        if let Some(item) = transform(path, contents) {
                            items.lock().unwrap().push(item);
                        }
                    }
                }

                ignore::WalkState::Continue
            })
        });
    }

    WalkResult { items: items.into_inner().unwrap() }
}

/// Walk directories in parallel, read files, transform, and report progress.
///
/// Similar to `parallel_read_transform` but calls `on_progress` after each file.
/// The progress callback receives the current count of processed files.
///
/// # Example
///
/// ```ignore
/// use stdx::fs::{parallel_read_transform_with_progress, WalkConfig};
/// use std::sync::atomic::{AtomicUsize, Ordering};
///
/// let progress = AtomicUsize::new(0);
/// let parsed = parallel_read_transform_with_progress(
///     &["/path/to/config"],
///     &WalkConfig {
///         extensions: &["xml"],
///         ..Default::default()
///     },
///     |path, contents| parse_xml(&contents).ok(),
///     || {
///         let count = progress.fetch_add(1, Ordering::Relaxed) + 1;
///         println!("Processed {} files", count);
///     },
/// );
/// ```
pub fn parallel_read_transform_with_progress<T, F, P>(
    roots: &[&Path],
    config: &WalkConfig<'_>,
    transform: F,
    on_progress: P,
) -> WalkResult<T>
where
    T: Send,
    F: Fn(&Path, Vec<u8>) -> Option<T> + Sync,
    P: Fn() + Sync,
{
    let items = std::sync::Mutex::new(Vec::new());

    for root in roots {
        let mut builder = WalkBuilder::new(root);
        builder.follow_links(config.follow_links).standard_filters(false).hidden(false);

        let excludes = config.excludes;
        let extensions = config.extensions;

        builder.build_parallel().run(|| {
            Box::new(|entry| {
                let Ok(entry) = entry else {
                    return ignore::WalkState::Continue;
                };

                let path = entry.path();

                // Check excludes
                if excludes.iter().any(|ex| path.starts_with(ex)) {
                    return ignore::WalkState::Skip;
                }

                // Skip non-files
                let Some(file_type) = entry.file_type() else {
                    return ignore::WalkState::Continue;
                };
                if !file_type.is_file() {
                    return ignore::WalkState::Continue;
                }

                // Check extension
                let ext = path.extension().and_then(|e| e.to_str());
                if ext.is_some_and(|x| extensions.iter().any(|e| e.eq_ignore_ascii_case(x))) {
                    if let Ok(contents) = std::fs::read(path) {
                        if let Some(item) = transform(path, contents) {
                            items.lock().unwrap().push(item);
                        }
                    }
                    on_progress();
                }

                ignore::WalkState::Continue
            })
        });
    }

    WalkResult { items: items.into_inner().unwrap() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parallel_count_empty() {
        let count = parallel_count(&[], &WalkConfig::default());
        assert_eq!(count, 0);
    }

    #[test]
    fn test_walk_config_default() {
        let config = WalkConfig::default();
        assert!(config.extensions.is_empty());
        assert!(config.excludes.is_empty());
        assert!(!config.follow_links);
    }

    #[test]
    fn test_walk_result_methods() {
        let result = WalkResult { items: vec![1, 2, 3] };
        assert_eq!(result.len(), 3);
        assert!(!result.is_empty());
        assert_eq!(result.into_vec(), vec![1, 2, 3]);
    }

    #[test]
    fn parallel_count_cancellable_quits_when_pre_set() {
        // Sanity: the crate's own src/ has at least a handful of .rs files,
        // so an uncancelled count is non-zero and lets us spot regressions
        // where the cancel flag silently does nothing.
        let here = Path::new(env!("CARGO_MANIFEST_DIR"));
        let baseline = parallel_count(
            &[here],
            &WalkConfig { extensions: &["rs"], excludes: &[], follow_links: false },
        );
        assert!(baseline >= 1, "expected at least one .rs file under stdx/, got {baseline}");

        // With cancel pre-set, every worker quits on its first callback
        // invocation — before reaching the `fetch_add` for matching files —
        // so the result must be exactly zero.
        let cancel = AtomicBool::new(true);
        let cancelled = parallel_count_cancellable(
            &[here],
            &WalkConfig { extensions: &["rs"], excludes: &[], follow_links: false },
            Some(&cancel),
        );
        assert_eq!(cancelled, 0, "pre-set cancel must short-circuit before any fetch_add");
    }

    #[test]
    fn parallel_count_cancellable_matches_uncancelled_when_disabled() {
        let here = Path::new(env!("CARGO_MANIFEST_DIR"));
        let cfg = WalkConfig { extensions: &["rs"], excludes: &[], follow_links: false };
        let plain = parallel_count(&[here], &cfg);
        let with_none = parallel_count_cancellable(&[here], &cfg, None);
        assert_eq!(plain, with_none);
    }
}
