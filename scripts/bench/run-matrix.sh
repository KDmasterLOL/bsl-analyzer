#!/usr/bin/env bash
# Runs every point of a bench manifest R times, one fresh process per
# (point, replicate) — the process-cold isolation the measurement plan
# requires. Raw per-run JSON reports land in OUT_DIR; nothing is aggregated
# here (statistics are a comparison-tool concern).
#
# Usage:
#   scripts/bench/run-matrix.sh -s <workspace> -m <manifest.json> -o <out-dir> \
#       [-r <replicates>] [-w <warm-iterations>] [-b <binary>]

set -euo pipefail

REPLICATES=5
WARM_ITERATIONS=20
BINARY=""
SOURCE_DIR=""
MANIFEST=""
OUT_DIR=""

while getopts "s:m:o:r:w:b:h" opt; do
  case "$opt" in
    s) SOURCE_DIR="$OPTARG" ;;
    m) MANIFEST="$OPTARG" ;;
    o) OUT_DIR="$OPTARG" ;;
    r) REPLICATES="$OPTARG" ;;
    w) WARM_ITERATIONS="$OPTARG" ;;
    b) BINARY="$OPTARG" ;;
    h)
      grep '^#' "$0" | sed 's/^# \{0,1\}//'
      exit 0
      ;;
    *) exit 2 ;;
  esac
done

if [[ -z "$SOURCE_DIR" || -z "$MANIFEST" || -z "$OUT_DIR" ]]; then
  echo "run-matrix: -s, -m and -o are required (see -h)" >&2
  exit 2
fi
if [[ -z "$BINARY" ]]; then
  BINARY="$(dirname "$0")/../../target/release/bsl-analyzer-app"
fi
if [[ ! -x "$BINARY" ]]; then
  echo "run-matrix: binary not found or not executable: $BINARY" >&2
  echo "run-matrix: build it with: cargo build --release" >&2
  exit 2
fi
command -v jq >/dev/null || { echo "run-matrix: jq is required" >&2; exit 2; }
if [[ "$REPLICATES" -lt 1 || "$WARM_ITERATIONS" -lt 1 ]]; then
  echo "run-matrix: -r and -w must be >= 1" >&2
  exit 2
fi

mkdir -p "$OUT_DIR"

# jq failures inside a process substitution are invisible to pipefail — run it
# separately so a malformed manifest is a hard error, not an empty point list.
if ! POINTS_RAW="$(jq -r '.targets[].id' "$MANIFEST")"; then
  echo "run-matrix: cannot read target ids from $MANIFEST" >&2
  exit 2
fi
mapfile -t POINTS <<<"$POINTS_RAW"
if [[ "${#POINTS[@]}" -eq 0 || -z "${POINTS[0]}" ]]; then
  echo "run-matrix: manifest has no targets" >&2
  exit 2
fi

echo "run-matrix: ${#POINTS[@]} points x $REPLICATES replicates -> $OUT_DIR" >&2

FAILURES=0
for point in "${POINTS[@]}"; do
  safe_name="${point//\//_}"
  for rep in $(seq 1 "$REPLICATES"); do
    out="$OUT_DIR/${safe_name}.r${rep}.json"
    log="$OUT_DIR/${safe_name}.r${rep}.log"
    if "$BINARY" bench run \
        --source-dir "$SOURCE_DIR" \
        --manifest "$MANIFEST" \
        --point "$point" \
        --mode latency \
        --warm-iterations "$WARM_ITERATIONS" \
        --json "$out" 2>"$log"; then
      rm -f "$log"
    else
      code=$?
      FAILURES=$((FAILURES + 1))
      echo "run-matrix: FAIL exit=$code point=$point rep=$rep (log: $log)" >&2
    fi
  done
done

if [[ "$FAILURES" -gt 0 ]]; then
  echo "run-matrix: $FAILURES failed run(s)" >&2
  exit 1
fi
echo "run-matrix: all runs completed" >&2
