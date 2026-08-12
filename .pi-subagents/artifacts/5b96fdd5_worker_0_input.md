# Task for worker

[Read from: /home/itrous/src/tools_migration/lsp/bsl-analyzer/context.md, /home/itrous/src/tools_migration/lsp/bsl-analyzer/plan.md]

You are a delegated subagent running from a fork of the parent session. Treat the inherited conversation as reference-only context, not a live thread to continue. Do not continue or answer prior messages as if they are waiting for a reply. Your sole job is to execute the task below and return a focused result for that task using your tools.

Task:
Fix the confirmed final OneCPI HIGH on the current issue #209 branch: one invalid-UTF8 or otherwise unreadable .bsl returned by a clean SourceSet scan must not abort the entire root transition forever.
Design a truthful partial-file contract: scan completeness and per-file read failure are distinct. The transition may publish the new root table while preserving any old carrier for an unread surviving key when safely attributable, and must create a durable dirty/unread retry obligation for a newly added or rebound unread key so watcher/overlay retry can heal it. It must not treat unread content as deletion, must not tombstone/hide a present unread baseline file, and must not claim its lexical/semantic content rebuilt.
Extend WorkspaceRootsTransitionPlan and cache/store transition inputs as needed with unread keys. Revalidation must compare the complete key set including unread files and distinguish a still-unread same file from create/delete/retarget. A read that heals or changes between plan and validation should supersede/retry from fresh bytes.
Add deterministic tests for invalid UTF-8 in a stable configuration file while adding an extension, invalid UTF-8 in the newly added root, preservation of an existing row/overlay entry for unread surviving keys, no baseline hiding/tombstone for unread-present, and later successful dirty refresh clearing the obligation.
Do not broaden into HNSW or selective-scan optimization unless required. Run fmt, bsl-search tests, mcp-server tests and clippy -D warnings. Leave changes uncommitted for review. No worktree/nested agents.

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