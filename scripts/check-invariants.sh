#!/usr/bin/env bash

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

ALLOW_MARKER='allow: keyword docs'

if command -v rg >/dev/null 2>&1; then
    HAVE_RG=1
else
    HAVE_RG=0
fi

overall_status=0


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

        trimmed="${text#"${text%%[![:space:]]*}"}"
        if [[ "$trimmed" == "//"* ]] || [[ "$trimmed" == "*"* ]]; then
            continue
        fi
        pd_real_calls=$(( pd_real_calls + 1 ))

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
