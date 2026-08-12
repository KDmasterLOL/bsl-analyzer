Implemented off-lock validation for issue #209 integration.

Changed files:
- `crates/bsl-search/src/engine.rs`
- `crates/bsl-search/src/lib.rs`
- `crates/bsl-search/src/workspace_overlay.rs`
- `crates/mcp-server/src/state/embed.rs`
- Existing integration changes remain in bootstrap, status, and baseline publisher.

Key fixes:
- Added `WorkspaceRootsTransitionPlan::revalidate()` and opaque validated-plan type.
- Guarded apply no longer performs filesystem scans or reads.
- Production validates before acquiring `SharedSearchEngine`.
- Documented watcher-backed post-validation race handling.
- Provider replacement now preserves lexical entries but advances wholesale fence.
- Applied transitions retain embedding signals even if provider installation fails.
- Incomplete validation retains last-known-good state and existing root-only retry obligation.
- Added deterministic mutex-observer test.

Validation:
- `cargo test -p bsl-search --lib`: 374 passed, 28 ignored.
- `cargo test -p mcp-server`: 700 tests passed, 1 ignored across unit/integration suites.
- `cargo clippy -p bsl-search -p mcp-server --all-targets --all-features -- -D warnings`: passed.
- Formatting and `git diff --check`: passed.
- No files staged or committed.

Open risk: live PostgreSQL tests still require `BSL_TEST_PG_URL`.