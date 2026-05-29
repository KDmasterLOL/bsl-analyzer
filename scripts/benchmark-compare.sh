#!/bin/bash


set -euo pipefail

PROJECT_DIR="${1:?Использование: $0 <путь-к-проекту> [конфиг]}"
PROJECT_DIR="$(cd "$PROJECT_DIR" && pwd)"
CONFIG="${2:-$PROJECT_DIR/.bsl-benchmark.json}"
SOURCE_DIR="./src/cf"
OUTPUT_DIR="/tmp/bsl-benchmark-output"

BSL_ANALYZER="${BSL_ANALYZER:-bsl-analyzer}"
BSL_LS="${BSL_LS:-bsl-language-server}"

RUNS="${RUNS:-1}"

if [ ! -d "$PROJECT_DIR/src/cf" ]; then
    echo "ОШИБКА: Директория $PROJECT_DIR/src/cf не найдена"
    exit 1
fi

if [ ! -f "$CONFIG" ]; then
    echo "ОШИБКА: Конфиг $CONFIG не найден"
    exit 1
fi

cd "$PROJECT_DIR"

if ! command -v "$BSL_ANALYZER" &>/dev/null; then
    echo "ОШИБКА: $BSL_ANALYZER не найден в PATH"
    exit 1
fi

if ! command -v "$BSL_LS" &>/dev/null; then
    echo "ОШИБКА: $BSL_LS не найден в PATH"
    exit 1
fi

if ! /usr/bin/time --version &>/dev/null 2>&1; then
    echo "ОШИБКА: /usr/bin/time (GNU time) не найден"
    exit 1
fi

BSL_COUNT=$(find "$PROJECT_DIR/src/cf" -name "*.bsl" | wc -l)
BSL_SIZE=$(find "$PROJECT_DIR/src/cf" -name "*.bsl" -print0 | xargs -0 cat 2>/dev/null | wc -c)
BSL_SIZE_MB=$(echo "scale=1; $BSL_SIZE / 1024 / 1024" | bc)

echo "================================================================"
echo "  БЕНЧМАРК: bsl-analyzer vs bsl-language-server"
echo "================================================================"
echo ""
echo "  Проект:      $PROJECT_DIR"
echo "  Источник:    $SOURCE_DIR"
echo "  Конфиг:      $CONFIG"
echo "  BSL файлов:  $BSL_COUNT"
echo "  Размер BSL:  ${BSL_SIZE_MB} MB"
echo "  Прогонов:    $RUNS"
echo ""
echo "================================================================"

mkdir -p "$OUTPUT_DIR"

run_benchmark() {
    local name="$1"
    shift
    local time_output="/tmp/bsl-bench-time-$$"

    echo ""
    echo "--- $name ---"
    echo "  Команда: $*"
    echo ""

    /usr/bin/time -v -o "$time_output" "$@" 2>/dev/null
    local exit_code=$?

    local wall_time=$(grep "Elapsed (wall clock) time" "$time_output" | sed 's/.*: //')
    local user_time=$(grep "User time" "$time_output" | awk '{print $NF}')
    local sys_time=$(grep "System time" "$time_output" | awk '{print $NF}')
    local max_rss=$(grep "Maximum resident set size" "$time_output" | awk '{print $NF}')
    local cpu_pct=$(grep "Percent of CPU" "$time_output" | awk '{print $NF}')
    local vol_ctx=$(grep "Voluntary context switches" "$time_output" | awk '{print $NF}')
    local invol_ctx=$(grep "Involuntary context switches" "$time_output" | awk '{print $NF}')

    local rss_mb=$(echo "scale=1; $max_rss / 1024" | bc)

    local wall_seconds
    if echo "$wall_time" | grep -qE "^[0-9]+:[0-9]+:[0-9]"; then
        wall_seconds=$(echo "$wall_time" | awk -F: '{print $1*3600 + $2*60 + $3}')
    elif echo "$wall_time" | grep -qE "^[0-9]+:[0-9]"; then
        wall_seconds=$(echo "$wall_time" | awk -F: '{print $1*60 + $2}')
    else
        wall_seconds="$wall_time"
    fi

    local files_per_sec=$(echo "scale=0; $BSL_COUNT / $wall_seconds" | bc 2>/dev/null || echo "N/A")

    echo "  РЕЗУЛЬТАТЫ:"
    echo "    Wall time:        $wall_time ($wall_seconds s)"
    echo "    User time:        ${user_time}s"
    echo "    System time:      ${sys_time}s"
    echo "    CPU:              $cpu_pct"
    echo "    Peak RSS:         ${rss_mb} MB ($max_rss kB)"
    echo "    Files/sec:        $files_per_sec"
    echo "    Context switches: $vol_ctx vol / $invol_ctx invol"
    echo "    Exit code:        $exit_code"

    eval "${name//[- ]/_}_wall=$wall_seconds"
    eval "${name//[- ]/_}_user=$user_time"
    eval "${name//[- ]/_}_sys=$sys_time"
    eval "${name//[- ]/_}_rss=$rss_mb"
    eval "${name//[- ]/_}_cpu=$cpu_pct"
    eval "${name//[- ]/_}_fps=$files_per_sec"

    rm -f "$time_output"
    return $exit_code
}

echo ""
echo "================================================================"
echo "  1/2: bsl-language-server"
echo "================================================================"

for i in $(seq 1 "$RUNS"); do
    [ "$RUNS" -gt 1 ] && echo "  --- Прогон $i/$RUNS ---"
    rm -rf "$OUTPUT_DIR/bsl-ls"
    mkdir -p "$OUTPUT_DIR/bsl-ls"
    run_benchmark "bsl_ls" \
        "$BSL_LS" analyze \
        -c="$CONFIG" \
        -o="$OUTPUT_DIR/bsl-ls" \
        -r=json \
        -s="$SOURCE_DIR" || true
done

echo ""
echo "================================================================"
echo "  2/2: bsl-analyzer"
echo "================================================================"

for i in $(seq 1 "$RUNS"); do
    [ "$RUNS" -gt 1 ] && echo "  --- Прогон $i/$RUNS ---"
    rm -rf "$OUTPUT_DIR/bsl-analyzer"
    mkdir -p "$OUTPUT_DIR/bsl-analyzer"
    run_benchmark "bsl_analyzer" \
        "$BSL_ANALYZER" analyze \
        -s "$SOURCE_DIR" \
        -c "$CONFIG" \
        -o "$OUTPUT_DIR/bsl-analyzer" \
        -r json \
        --streaming \
        -q || true
done

echo ""
echo ""
echo "================================================================"
echo "  СРАВНЕНИЕ"
echo "================================================================"
echo ""

if [ -n "${bsl_ls_wall:-}" ] && [ -n "${bsl_analyzer_wall:-}" ]; then
    speed_ratio=$(echo "scale=2; $bsl_ls_wall / $bsl_analyzer_wall" | bc 2>/dev/null || echo "N/A")
    mem_ratio=$(echo "scale=2; $bsl_ls_rss / $bsl_analyzer_rss" | bc 2>/dev/null || echo "N/A")
    user_ratio=$(echo "scale=2; $bsl_ls_user / $bsl_analyzer_user" | bc 2>/dev/null || echo "N/A")

    printf "%-20s  %15s  %15s  %12s\n" "Метрика" "bsl-ls" "bsl-analyzer" "Разница"
    printf "%-20s  %15s  %15s  %12s\n" "--------------------" "---------------" "---------------" "------------"
    printf "%-20s  %15ss  %15ss  %10sx\n" "Wall time" "$bsl_ls_wall" "$bsl_analyzer_wall" "$speed_ratio"
    printf "%-20s  %15ss  %15ss  %10sx\n" "User time (CPU)" "$bsl_ls_user" "$bsl_analyzer_user" "$user_ratio"
    printf "%-20s  %15ss  %15ss\n"       "System time" "$bsl_ls_sys" "$bsl_analyzer_sys"
    printf "%-20s  %14sMB  %14sMB  %10sx\n" "Peak RSS" "$bsl_ls_rss" "$bsl_analyzer_rss" "$mem_ratio"
    printf "%-20s  %15s  %15s\n"         "CPU usage" "$bsl_ls_cpu" "$bsl_analyzer_cpu"
    printf "%-20s  %15s  %15s\n"         "Files/sec" "$bsl_ls_fps" "$bsl_analyzer_fps"
fi

echo ""
echo "Отчёты: $OUTPUT_DIR"
echo ""
