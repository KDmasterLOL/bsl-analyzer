# Task for worker

[Read from: /tmp/pi-worktree-bfbb71ad-0/context.md, /tmp/pi-worktree-bfbb71ad-0/plan.md]

You are a delegated subagent running from a fork of the parent session. Treat the inherited conversation as reference-only context, not a live thread to continue. Do not continue or answer prior messages as if they are waiting for a reply. Your sole job is to execute the task below and return a focused result for that task using your tools.

Task:
Implement the graph-side protocol required by issue #209 in your isolated worktree, scoped to crates/mcp-server/src/graph plus the exact config trigger in state/sync.rs if needed, but DO NOT implement the SearchEngine root transition or state/embed orchestration. Requirements: (1) add a forced full-project reload entry point for an exact analyzer config in the workspace root; it must bypass GraphFp equality, skip body-only incremental reload, survive an in-flight build/reload, and clear its durable obligation only after a full publication; nested same-basename configs must not trigger it. (2) Make the graph publish hook outcome structured with independent topology_handled and roots_handled flags; GraphPublishSignal carries roots_refresh_requested; every current successful publish/adopt invokes the hook with root checking requested, including fingerprint/topology-equal publications. (3) Add independent pending root-refresh obligation and flush_pending_search_roots_refresh, without turning root-only retry into topology_changed=true. (4) Add deterministic unit tests for alias/root-config edit with unchanged GraphFp forcing full publish, publish-hook coverage, separate retries, and nested config negative. Preserve existing topology semantics. Update call sites/tests to compile using vacuous/default structured outcomes where root orchestration is not implemented yet. Run fmt and cargo test -p mcp-server graph-related tests (or full crate if practical), commit green Conventional Commit, and report commit hash, changed files, commands and residual risks.

---
Update progress at: /home/itrous/src/tools_migration/lsp/bsl-analyzer/.pi-subagents/artifacts/progress/bfbb71ad/progress.md

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