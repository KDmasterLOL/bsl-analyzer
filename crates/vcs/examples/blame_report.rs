//! Timing harness for git2 blame on a real repository.
//!
//! Usage:
//!   cargo run --release -p vcs --example blame_report -- <repo> <file> [<file>...]
//!
//! Blames each file twice (cold and warm) and prints per-author line counts,
//! so the cost of a per-file blame pass can be measured before building the
//! `ignored_authors` filter on top of it.

use std::collections::HashMap;
use std::path::Path;
use std::time::Instant;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [repo_path, files @ ..] = args.as_slice() else {
        eprintln!("usage: blame_report <repo> <file> [<file>...]");
        std::process::exit(2);
    };

    let repo = git2::Repository::discover(repo_path)?;

    for file in files {
        for pass in ["cold", "warm"] {
            let start = Instant::now();
            let mut opts = git2::BlameOptions::new();
            opts.first_parent(true);
            let blame = repo.blame_file(Path::new(file), Some(&mut opts))?;
            let blame_elapsed = start.elapsed();

            let buffer_start = Instant::now();
            let workdir = repo.workdir().expect("bare repositories are not supported");
            let contents = std::fs::read(workdir.join(file))?;
            let buffer_blame = blame.blame_buffer(&contents)?;
            let buffer_elapsed = buffer_start.elapsed();

            let mut lines_by_author: HashMap<String, usize> = HashMap::new();
            let mut uncommitted = 0usize;
            for hunk in buffer_blame.iter() {
                if hunk.final_commit_id().is_zero() {
                    uncommitted += hunk.lines_in_hunk();
                    continue;
                }
                // `blame_buffer` hunks carry null signatures (git2 would hit
                // a null pointer), so authorship comes from the commit object.
                let author = repo
                    .find_commit(hunk.final_commit_id())
                    .ok()
                    .and_then(|c| c.author().name().map(str::to_owned))
                    .unwrap_or_else(|| "<unknown>".to_owned());
                *lines_by_author.entry(author).or_default() += hunk.lines_in_hunk();
            }

            let mut authors: Vec<_> = lines_by_author.into_iter().collect();
            authors.sort_by_key(|(_, lines)| std::cmp::Reverse(*lines));
            let top: Vec<String> =
                authors.iter().take(3).map(|(name, lines)| format!("{name}: {lines}")).collect();

            println!(
                "{pass}: blame {:?} + buffer {:?} — {} ({} hunks, {} uncommitted lines; top: {})",
                blame_elapsed,
                buffer_elapsed,
                file,
                blame.len(),
                uncommitted,
                top.join(", "),
            );
        }
    }

    Ok(())
}
