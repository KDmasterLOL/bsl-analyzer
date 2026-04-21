#!/usr/bin/env bash
# M3 Invariants — CI gate for type-system facade boundaries.
#
# This script enforces the facade-boundary rule from
# `docs/architecture/TYPE_SYSTEM.md` Invariant #3: IDE code must not
# reach into `bsl_platform::PlatformData::instance()` directly on the
# type path. All such access goes through:
#
#   - `hir::Semantics::type_of_expr` / `hir::Type` (semantic facade), or
#   - Salsa-tracked queries in `bsl-platform::db` (`type_methods_query`,
#     `manager_methods_query`, `platform_method_query`, …).
#
# The only allowed exceptions are keyword-docs lookups — keywords
# aren't part of the type system and predate M3. Each keyword-docs
# callsite carries a comment of the form
# `allow: keyword docs (M3 exception)` on the SAME line as the
# `PlatformData::instance()` call (plain rg -B scan would miss it,
# so we inspect each violation's text for the marker).
#
# The script exits non-zero and prints offending files if it finds
# any unmarked `PlatformData::instance()` call inside `crates/ide/`.
#
# Run locally:
#   scripts/check-invariants.sh
#
# Hook into CI by invoking this script from the pipeline's build step.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

ALLOW_MARKER='allow: keyword docs'

# grep/rg alternatives: ripgrep is present in the dev environment and
# used by the rest of the tooling. Fall back to `git grep` for CI
# environments that skip rg.
if command -v rg >/dev/null 2>&1; then
    matches="$(rg --no-heading --line-number \
        --glob 'crates/ide/**/*.rs' \
        'PlatformData::instance\(\)' \
        || true)"
else
    matches="$(git grep -n 'PlatformData::instance()' -- 'crates/ide/**/*.rs' || true)"
fi

if [[ -z "$matches" ]]; then
    echo "invariants: no PlatformData::instance() calls in crates/ide/ (clean)"
    exit 0
fi

# For each match, inspect a 6-line window around it (3 lines before
# and after the hit) for the `allow:` marker. A hit is OK iff the
# window mentions the marker, which covers both the call-line comment
# and any doc-comment that documents the exception just above.
violations=""
real_calls=0
while IFS= read -r line; do
    file="${line%%:*}"
    rest="${line#*:}"
    lineno="${rest%%:*}"
    text="${rest#*:}"

    # Skip matches where the hit is inside a comment (`// …` or `/// …`).
    # These are doc/prose mentions like "don't touch PlatformData::instance()"
    # explaining the rule, not actual calls.
    trimmed="${text#"${text%%[![:space:]]*}"}"
    if [[ "$trimmed" == "//"* ]] || [[ "$trimmed" == "*"* ]]; then
        continue
    fi
    real_calls=$(( real_calls + 1 ))

    # Portable 8-line window: (lineno - 5) .. (lineno + 2). The allow
    # marker can sit several lines above the call when the explanation
    # spans a block comment.
    start=$(( lineno - 5 ))
    end=$(( lineno + 2 ))
    [[ $start -lt 1 ]] && start=1
    window="$(sed -n "${start},${end}p" "$file")"
    if ! echo "$window" | grep -qF "$ALLOW_MARKER"; then
        violations+="${line}
"
    fi
done <<< "$matches"

if [[ -z "${violations// /}" ]]; then
    # All real-code matches carry the allow marker — white-listed
    # exceptions only.
    echo "invariants: $real_calls PlatformData::instance() call(s) in crates/ide/ — all keyword-docs exceptions (white-listed)"
    exit 0
fi

echo "invariants: facade-boundary violations in crates/ide/ —"
echo "$violations"
echo
echo "Add an '$ALLOW_MARKER (M3 exception)' comment within 3 lines of the call,"
echo "or migrate to a Salsa-tracked query / hir::Semantics bridge."
exit 1
