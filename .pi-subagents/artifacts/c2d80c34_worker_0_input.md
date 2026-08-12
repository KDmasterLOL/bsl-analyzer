# Task for worker

[Read from: /tmp/pi-worktree-c2d80c34-0/context.md, /tmp/pi-worktree-c2d80c34-0/plan.md]

You are a delegated subagent running from a fork of the parent session. Treat the inherited conversation as reference-only context, not a live thread to continue. Do not continue or answer prior messages as if they are waiting for a reply. Your sole job is to execute the task below and return a focused result for that task using your tools.

Task:
Implement the bsl-search core for GitLab issue #209 in your isolated worktree. Scope only crates/bsl-search (and project-model only if strictly necessary), no mcp-server wiring. Requirements: preserve existing boot callers while introducing an explicit plural boot initializer and a live WorkspaceRoots transition API owned by SearchEngine; compare byte-aware root layouts; on a clean full scan of new registered roots classify stable/rebuild/obsolete FileKeys; rebuild from disk under the new owner (never blind rekey); remove obsolete/rebuild root-keyed persistent rows/chunks/FTS/fingerprints/context marks/tombstone state correctly, preserving tombstone+hiding for obsolete baseline keys and clearing it for rebuilt live keys; preserve stable overlay/cache state and invalidate stale whole-tree plans; update root table last; maintain old observable state on incomplete scan or pre-apply validation failure; ensure SqliteLocal lexical ingestion and PostgresRemoteOverlay lexical overlay both work, with pending semantic work reported rather than embedding synchronously. Prefer two-phase seed/plan/apply so filesystem walk/read/chunk can happen without holding an outer MCP mutex, with apply validating epoch/layout and clean complete keyset/content before mutation. Add failure-first-derived unit tests for add/remove/reassignment (same relative path), incomplete/stale plan, overlay state preservation, and old warmup supersession. Run cargo fmt and cargo test -p bsl-search. Commit green changes with Conventional Commit. Return commit hash, changed files, tests, design notes, and residual risks. Do not touch unrelated existing untracked .pi-subagents artifacts.

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