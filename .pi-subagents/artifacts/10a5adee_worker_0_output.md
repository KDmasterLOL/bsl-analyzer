Implemented production orchestration for issue #209.

Changed files:
- `crates/bsl-search/src/engine.rs`
- `crates/mcp-server/src/state/embed.rs`
- `crates/mcp-server/src/state/bootstrap.rs`
- `crates/mcp-server/src/tools/search/status.rs`
- `crates/bsl-analyzer/src/bin/cli/search_baseline/documents.rs`

Key changes:
- Publish hook now applies same-snapshot roots before context refresh.
- Planning runs outside the engine mutex using the verified published `GraphDb` provider.
- `Applied` outcomes kick existing local `EmbedFlight` or PG `OverlayRetry`.
- Unchanged publications use an O(1) roots comparison without rescanning.
- Bootstrap and baseline publisher use plural boot initialization.
- Status reports current search roots.
- Added production-hook live add/remove regression test.

Validation:
- `cargo test -p mcp-server`: 668 unit + 31 integration tests passed; 1 ignored.
- `cargo test -p bsl-search`: 373 passed; 28 ignored.
- Baseline publisher tests: 11 passed.
- Workspace clippy with `-D warnings`: passed.
- Working diff is formatted, unstaged, and uncommitted as requested.

Open risks/questions:
- Live Postgres tests require `BSL_TEST_PG_URL` and remain ignored.
- Mandatory main-session OneCPI review is still pending.

Recommended next step: run OneCPI review over the uncommitted integration diff, fix blockers, then commit the landing.