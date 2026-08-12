Implemented the partial-file root-transition contract for unread/invalid-UTF8 `.bsl` files.

Changed files:
- `crates/bsl-search/src/engine.rs`
- `crates/bsl-search/src/workspace_overlay.rs`

Validation:
- `bsl-search`: 379 passed, 28 ignored
- `mcp-server`: 700 passed, 1 ignored
- Workspace clippy with `-D warnings`: passed
- Formatting and `git diff --check`: passed
- No staged files

Open risks/questions:
- Live PostgreSQL tests remain ignored without `BSL_TEST_PG_URL`.
- Requested `context.md` and `plan.md` were absent; implementation followed the supplied task and existing branch design.
- Changes intentionally remain uncommitted for review.

Recommended next step: run OneCPI review over this two-file diff.