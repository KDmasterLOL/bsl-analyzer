# Task for worker

[Read from: /home/itrous/src/tools_migration/lsp/bsl-analyzer/context.md, /home/itrous/src/tools_migration/lsp/bsl-analyzer/plan.md]

You are a delegated subagent running from a fork of the parent session. Treat the inherited conversation as reference-only context, not a live thread to continue. Do not continue or answer prior messages as if they are waiting for a reply. Your sole job is to execute the task below and return a focused result for that task using your tools.

Task:
Fix the current uncommitted issue #209 integration diff in this task branch after OneCPI review.
Confirmed HIGH: production refresh_search_roots_after_graph calls apply_workspace_roots_transition under SharedSearchEngine mutex, and apply performs a second full SourceSet scan plus fs::read/hash for every file, blocking searches.
Redesign the core API so expensive second validation happens off the outer MCP engine mutex, for example a public revalidate/validated-plan phase. Guarded apply under mutex must do only in-memory epoch/cache checks plus SQLite/cache/vector atomic mutation. Preserve a standalone safe API if needed, but production must use validated apply.
Document watcher-backed race semantics: the hub is rearmed before the hook; any create/modify/delete after off-lock validation is queued and, after apply, attributed via new roots. Add a deterministic test proving filesystem scan/read validation does not execute while the outer engine mutex is held, using a test seam/barrier/counter rather than sleeps.
Also address related review findings correctly: unchanged-root provider replacement must fence old overlay plans; do not silently swap provider. On incomplete scan keep last-known-good and retry via the existing obligation, without a busy loop. Preserve pending embedding signals even if provider installation fails after an Applied transition.
Critically inspect current replace_published_graph_context_provider design: stable overlay entries need not be discarded lexically, but stale semantic plans must be fenced and future point refreshes must use the new provider. Add focused tests.
Run cargo fmt, cargo test -p bsl-search, targeted/full cargo test -p mcp-server, and clippy with -D warnings as practical. Leave diff uncommitted for OneCPI rerun. Do not use wt/worktree or nested agents.

## Acceptance Contract
Acceptance level: checked
Completion is not accepted from prose alone. End with a structured acceptance report.

Criteria:
- criterion-1: Implement the requested change without widening scope

Required evidence: changed-files, tests-added, commands-run, residual-risks, no-staged-files, validation-output

Finish with a fenced JSON block tagged `acceptance-report` in this shape:
Use empty arrays when no items apply; array fields contain strings unless object entries are shown.
`criteriaSatisfied[].status` must be exactly one of: satisfied, not-satisfied, not-applicable.
`commandsRun[].result` must be exactly one of: passed, failed, not-run.
`manualNotes` and `notes` are optional strings; an empty string means no note and does not satisfy `manual-notes` evidence.
```acceptance-report
{
  "criteriaSatisfied": [
    {
      "id": "criterion-1",
      "status": "satisfied",
      "evidence": "specific proof"
    }
  ],
  "changedFiles": [
    "src/file.ts"
  ],
  "testsAddedOrUpdated": [
    "test/file.test.ts"
  ],
  "commandsRun": [
    {
      "command": "command",
      "result": "passed",
      "summary": "short result"
    }
  ],
  "validationOutput": [
    "validation output or concise summary"
  ],
  "residualRisks": [
    "none"
  ],
  "noStagedFiles": true,
  "diffSummary": "short description of the diff",
  "reviewFindings": [
    "blocker: file.ts:12 - issue found, or no blockers"
  ],
  "manualNotes": "anything else the parent should know"
}
```