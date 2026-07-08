You are triaging a closed Sonar issue for the bsl-analyzer project.

The attached JSON contains a Sonar issue snapshot, a small source snippet from the analyzed downstream project, and `existing_problems` — analyzer-problem groups already tracked in GitLab. The local working directory is the bsl-analyzer repository; inspect the analyzer implementation, diagnostic metadata, docs, and tests when useful.

Important constraints:

- Context may be incomplete. Do not invent a root cause when the snippet is insufficient.
- The goal is to preserve a useful signal for future diagnostic fixes, not to solve the bug completely now.
- Never mention tokens, environment files, or authentication details.
- Return only one compact JSON object. No markdown, no code fences, no prose outside JSON.

Required JSON schema:

{
  "confidence": "high | medium | low",
  "classification": "valid_false_positive | analyzer_gap | stale_issue | duplicate | unknown",
  "summary": "short Russian summary",
  "evidence": ["repo or snippet observations"],
  "unknowns": ["missing context, if any"],
  "problem_key": "stable-kebab-slug",
  "problem_title": "short Russian title of the analyzer problem",
  "recommended_gitlab_action": "create_issue | skip"
}

Grouping rules (problem_key / problem_title):

- `problem_key` identifies the ANALYZER PROBLEM CLASS, not this specific occurrence. Two closures caused by the same analyzer defect must share the same key, even in different projects, files, or with different identifier names.
- Describe the ROOT CAUSE, not the concrete symbol/file/number. Good: `unused-parameters-mandatory-event-handler-signature`. Bad: `unused-parameter-element` (identifier-specific), `magic-number-6` (value-specific).
- Reuse an EXISTING key verbatim from `existing_problems` when this closure is the same problem. Only coin a new key when none matches. Keep keys lowercase kebab-case, prefixed by the rule slug (e.g. `type-mismatch-by-doc-comment-...`).
- `problem_title` is a concise human title for that group (reuse the existing group's title when reusing its key).

Confidence rules:

- high: the snippet and repository code strongly suggest a specific analyzer behavior to fix.
- medium: there is a plausible hypothesis, but more downstream context may be needed.
- low: only the fact of closure is reliable; keep the issue as a needs-investigation signal.

For low confidence, prefer create_issue unless the Sonar issue is clearly irrelevant or duplicate.
