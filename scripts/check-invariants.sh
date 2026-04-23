#!/usr/bin/env bash
# M3/M4 Invariants — CI gate for type-system facade boundaries.
#
# ## Invariant #3 — `PlatformData::instance()` gate
#
# IDE code must not reach into `bsl_platform::PlatformData::instance()`
# directly on the type path. All such access goes through:
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
# ## Invariant (M4 Task 4) — `AttributeType → Ty` gate
#
# Semantic consumers must lower `bsl_metadata::AttributeType` through
# `TypeRef::from_attribute_type` + `TyLoweringContext::lower_type_ref`,
# not by pattern-matching `AttributeType` variants and producing `Ty`
# values inline. The anti-pattern this guards against is co-occurrence
# of `AttributeType::` and `Ty::` in a single semantic-layer file.
#
# Allowed (whitelist):
#   - `crates/bsl-metadata/**` — source of truth for `AttributeType`.
#   - `crates/hir-def/src/type_ref.rs` — the `TypeRef::from_attribute_type` bridge.
#   - `crates/sdbl-hir/src/types.rs` — SDBL's parallel type system bridge.
#   - `crates/sdbl-hir/src/lower/from_clause.rs` — SDBL `DefinedType` special-case.
#
# In-file test modules (`#[cfg(test)] mod tests { … }`) are stripped
# before the co-occurrence check — `AttributeType::X` constructors in
# test fixtures are a normal idiom and must not force whole production
# files onto the whitelist. Test-fixture files that live *outside* a
# production module (e.g. `crates/*/tests/`) are exempt by
# construction: they only contain `AttributeType::`, never `Ty::`.
#
# The script runs both gates, aggregates violations, and exits non-zero
# when either invariant is breached.
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
    HAVE_RG=1
else
    HAVE_RG=0
fi

overall_status=0

# ----------------------------------------------------------------------
# Gate 1 — PlatformData::instance() in crates/ide/
# ----------------------------------------------------------------------

if [[ $HAVE_RG -eq 1 ]]; then
    pd_matches="$(rg --no-heading --line-number \
        --glob 'crates/ide/**/*.rs' \
        'PlatformData::instance\(\)' \
        || true)"
else
    pd_matches="$(git grep -n 'PlatformData::instance()' -- 'crates/ide/**/*.rs' || true)"
fi

if [[ -z "$pd_matches" ]]; then
    echo "invariants: PlatformData::instance() gate — clean (no calls in crates/ide/)"
else
    pd_violations=""
    pd_real_calls=0
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
        pd_real_calls=$(( pd_real_calls + 1 ))

        # Portable 8-line window: (lineno - 5) .. (lineno + 2). The allow
        # marker can sit several lines above the call when the explanation
        # spans a block comment.
        start=$(( lineno - 5 ))
        end=$(( lineno + 2 ))
        [[ $start -lt 1 ]] && start=1
        window="$(sed -n "${start},${end}p" "$file")"
        if ! echo "$window" | grep -qF "$ALLOW_MARKER"; then
            pd_violations+="${line}
"
        fi
    done <<< "$pd_matches"

    if [[ -z "${pd_violations// /}" ]]; then
        echo "invariants: PlatformData::instance() gate — $pd_real_calls call(s), all keyword-docs exceptions (white-listed)"
    else
        echo "invariants: facade-boundary violations in crates/ide/ —"
        echo "$pd_violations"
        echo
        echo "Add an '$ALLOW_MARKER (M3 exception)' comment within 3 lines of the call,"
        echo "or migrate to a Salsa-tracked query / hir::Semantics bridge."
        echo
        overall_status=1
    fi
fi

# ----------------------------------------------------------------------
# Gate 2 — AttributeType → Ty co-occurrence
# ----------------------------------------------------------------------

# Files allowed to carry both tokens simultaneously (blessed adapters
# and the bridge). Any file outside this list that contains both
# `AttributeType::` and `Ty::` is flagged.
AT_WHITELIST=(
    'crates/bsl-metadata/'
    'crates/hir-def/src/type_ref.rs'
    'crates/sdbl-hir/src/types.rs'
    'crates/sdbl-hir/src/lower/from_clause.rs'
)

is_whitelisted() {
    local path="$1"
    for allowed in "${AT_WHITELIST[@]}"; do
        if [[ "$path" == "$allowed"* ]]; then
            return 0
        fi
    done
    return 1
}

# Find files under crates/ that contain `AttributeType::`.
if [[ $HAVE_RG -eq 1 ]]; then
    at_files="$(rg --no-heading --files-with-matches \
        --glob 'crates/**/*.rs' \
        'AttributeType::' \
        || true)"
else
    at_files="$(git grep -l 'AttributeType::' -- 'crates/**/*.rs' || true)"
fi

at_violations=""
at_flagged=0
at_skipped=0
if [[ -n "$at_files" ]]; then
    while IFS= read -r file; do
        [[ -z "$file" ]] && continue
        if is_whitelisted "$file"; then
            continue
        fi
        # Co-occurrence trigger on production, non-comment lines only:
        #
        #  1. Cut everything from the first top-level `#[cfg(test)]`
        #     attribute to EOF — idiomatic in this codebase for the
        #     end-of-file `mod tests { … }` block. Attribute-driven
        #     test fixtures legitimately mention `AttributeType::X`
        #     constructors and must not force the production half of
        #     the same file onto the whitelist. A file with an
        #     intermixed `#[cfg(test)] fn helper() …` before real
        #     production code would see that tail stripped too; no
        #     file in `crates/` uses that layout today (all files
        #     place `#[cfg(test)]` immediately before the trailing
        #     `mod tests`).
        #  2. Strip line comments (`//` and `///`) and block-comment
        #     `*` continuations, covering prose that names `Ty::` or
        #     `AttributeType::` without actually constructing them.
        stripped="$(awk '/^#\[cfg\(test\)\]/ { exit } { print }' "$file" \
            | sed -E 's|//.*$||' \
            | grep -vE '^[[:space:]]*(\*|$)')"
        if echo "$stripped" | grep -qF 'AttributeType::' \
           && echo "$stripped" | grep -qF 'Ty::'; then
            at_violations+="${file}
"
            at_flagged=$(( at_flagged + 1 ))
        else
            at_skipped=$(( at_skipped + 1 ))
        fi
    done <<< "$at_files"
fi

if [[ -z "${at_violations// /}" ]]; then
    echo "invariants: AttributeType → Ty gate — clean ($at_skipped test-fixture file(s) carrying only AttributeType::, no Ty:: co-occurrence)"
else
    echo "invariants: AttributeType → Ty violations —"
    echo "$at_violations"
    echo "The files above reference both AttributeType:: and Ty::, suggesting a"
    echo "direct match/convert pattern. Route AttributeType through"
    echo "TypeRef::from_attribute_type + TyLoweringContext::lower_type_ref,"
    echo "or add the file to AT_WHITELIST in scripts/check-invariants.sh if"
    echo "it is a new blessed adapter (see the doc comment at the top)."
    overall_status=1
fi

exit $overall_status
