# Task for worker

[Read from: /home/itrous/src/tools_migration/lsp/bsl-analyzer/context.md, /home/itrous/src/tools_migration/lsp/bsl-analyzer/plan.md]

You are a delegated subagent running from a fork of the parent session. Treat the inherited conversation as reference-only context, not a live thread to continue. Do not continue or answer prior messages as if they are waiting for a reply. Your sole job is to execute the task below and return a focused result for that task using your tools.

Task:
Implement issue #209 production root-transition orchestration on the current task branch, building on commits 1cc2c242 and c62532db. Fix the confirmed graph review blocker: build_publish_hook must actually apply signal.workspace_roots. Requirements: verify/open the just-published GraphDb topology and use an Arc<GraphDbContextProvider> from that exact artifact for transition planning; add a small bsl-search seed API to override the graph provider if needed. Sequence: if drift_pending defer roots; lock engine briefly to capture seed, unlock for seed.plan filesystem work, relock and apply; Unchanged/Applied = roots_handled true, Superseded/error = false with last-known-good intact. Root transition must happen before topology context refresh. Consume outcome: pending_collection_embeddings kicks existing EmbedFlight pending pass; pending_overlay_embeddings kicks existing OverlayRetry. Move/create OverlayRetry early enough to capture in hook, without adding a second worker. Root-only retry stays separate. Update search status wording from boot roots to current roots. Migrate production bootstrap and baseline publisher from compatibility set_workspace_roots to initialize_workspace_roots while preserving the baseline publisher full configuration+extensions table; keep test compatibility for now. Add deterministic production-hook tests for add/remove/reassignment, transient failure retry without new event, local semantic kick and PG lexical/no-embedder/overlay kick where practical. Run fmt, targeted tests, full cargo test -p mcp-server and clippy -D warnings as time permits. Do NOT commit yet: leave a green uncommitted diff for main-session OneCPI review. No wt/worktree/nested agents.

## Acceptance Contract
Acceptance level: checked
Completion is not accepted from prose alone. End with a structured acceptance report.

Criteria:
- criterion-1: Implement the requested change without widening scope
- criterion-2: Return evidence sufficient for an independent acceptance review

Required evidence: changed-files, tests-added, commands-run, residual-risks, no-staged-files, validation-output

Review gate: required by reviewer.

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
    },
    {
      "id": "criterion-2",
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