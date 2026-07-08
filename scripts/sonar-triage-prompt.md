You are triaging a closed Sonar issue for the bsl-analyzer project.

The attached JSON contains a Sonar issue snapshot and a small source snippet from the analyzed downstream project. The local working directory is the bsl-analyzer repository; inspect the analyzer implementation, diagnostic metadata, docs, and tests when useful.

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
  "recommended_gitlab_action": "create_issue | skip"
}

Confidence rules:

- high: the snippet and repository code strongly suggest a specific analyzer behavior to fix.
- medium: there is a plausible hypothesis, but more downstream context may be needed.
- low: only the fact of closure is reliable; keep the issue as a needs-investigation signal.

For low confidence, prefer create_issue unless the Sonar issue is clearly irrelevant or duplicate.
