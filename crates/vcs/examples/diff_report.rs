//! Timing harness for diff-report generation on a real repository.
//!
//! Usage:
//!   cargo run --release -p vcs --example diff_report -- <repo> <base> [<head>]
//!
//! With `<head>` given, runs the tree-to-tree mode; otherwise diffs the
//! working directory (plus index) against merge-base(base, HEAD). Runs the
//! computation twice to show cold and warm timings.

use std::time::Instant;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (repo, base, head) = match args.as_slice() {
        [repo, base] => (repo.clone(), base.clone(), None),
        [repo, base, head] => (repo.clone(), base.clone(), Some(head.clone())),
        _ => {
            eprintln!("usage: diff_report <repo> <base> [<head>]");
            std::process::exit(2);
        }
    };

    for pass in ["cold", "warm"] {
        let start = Instant::now();
        let diff = match &head {
            Some(head) => vcs::generate_diff_report(repo.as_ref(), &base, head, true)?,
            None => vcs::generate_workdir_diff_report(repo.as_ref(), &base, true)?,
        };
        let elapsed = start.elapsed();

        let whole_files =
            diff.report.files.values().filter(|change| change.hunks.is_none()).count();
        let total_hunks: usize = diff
            .report
            .files
            .values()
            .filter_map(|change| change.hunks.as_ref().map(Vec::len))
            .sum();

        println!(
            "{pass}: {:?} — {} changed .bsl files ({} whole-file, {} hunks) vs '{}'",
            elapsed,
            diff.report.files.len(),
            whole_files,
            total_hunks,
            diff.report.base_ref,
        );
    }

    Ok(())
}
