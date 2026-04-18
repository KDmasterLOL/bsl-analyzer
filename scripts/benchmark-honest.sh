#!/bin/bash
# Честное сравнение: bsl-analyzer vs bsl-language-server
# - 3 прогона каждого инструмента
# - Отдельные конфиги (JSON для bsl-ls, TOML для bsl-analyzer)
# - Все прогоны + медиана по метрикам
# - /usr/bin/time -v для замеров через getrusage()

set -euo pipefail

PROJECT_DIR="${1:?Использование: $0 <путь-к-проекту>}"
PROJECT_DIR="$(cd "$PROJECT_DIR" && pwd)"

BSL_LS_CONFIG="${BSL_LS_CONFIG:-/tmp/bsl-bench-configs/bsl-ls.json}"
BSL_ANALYZER_CONFIG="${BSL_ANALYZER_CONFIG:-/tmp/bsl-bench-configs/bsl-analyzer.toml}"
SOURCE_DIR="src/cf"
RUNS="${RUNS:-3}"

BSL_LS="${BSL_LS:-/home/itrous/bsl-language-server/bsl-language-server/bin/bsl-language-server}"
BSL_ANALYZER="${BSL_ANALYZER:-/home/itrous/src/tools_migration/lsp/bsl-analyzer/target/release/bsl-analyzer-app}"

OUTPUT_DIR="/tmp/bsl-benchmark-honest"
RESULTS_DIR="$OUTPUT_DIR/results"

# --- Проверки ---
[ -d "$PROJECT_DIR/$SOURCE_DIR" ] || { echo "ОШИБКА: $PROJECT_DIR/$SOURCE_DIR не найдена"; exit 1; }
[ -f "$BSL_LS_CONFIG" ] || { echo "ОШИБКА: $BSL_LS_CONFIG не найден"; exit 1; }
[ -f "$BSL_ANALYZER_CONFIG" ] || { echo "ОШИБКА: $BSL_ANALYZER_CONFIG не найден"; exit 1; }
[ -x "$BSL_LS" ] || { echo "ОШИБКА: $BSL_LS не найден/неисполняем"; exit 1; }
[ -x "$BSL_ANALYZER" ] || { echo "ОШИБКА: $BSL_ANALYZER не найден/неисполняем"; exit 1; }

# --- Подсчёт файлов ---
BSL_COUNT=$(find "$PROJECT_DIR/$SOURCE_DIR" -name "*.bsl" | wc -l)
BSL_SIZE=$(find "$PROJECT_DIR/$SOURCE_DIR" -name "*.bsl" -print0 | xargs -0 cat 2>/dev/null | wc -c)
BSL_SIZE_MB=$(echo "scale=1; $BSL_SIZE / 1024 / 1024" | bc)

BSL_LS_VERSION=$("$BSL_LS" version 2>&1 | grep "^version:" | awk '{print $2}')
BSL_ANALYZER_VERSION=$("$BSL_ANALYZER" --version 2>&1 | head -1)

JVM_XMX=$(grep -oP 'Xmx\S+' /home/itrous/bsl-language-server/bsl-language-server/lib/app/bsl-language-server.cfg || echo "?")

echo "================================================================"
echo "  ЧЕСТНЫЙ БЕНЧМАРК: bsl-analyzer vs bsl-language-server"
echo "================================================================"
echo ""
echo "  Проект:          $PROJECT_DIR"
echo "  Источник:        $SOURCE_DIR"
echo "  BSL файлов:      $BSL_COUNT"
echo "  Размер BSL:      ${BSL_SIZE_MB} MB"
echo "  Прогонов:        $RUNS"
echo ""
echo "  bsl-ls:          $BSL_LS_VERSION (JVM $JVM_XMX)"
echo "  bsl-analyzer:    $BSL_ANALYZER_VERSION"
echo ""
echo "  Конфиг bsl-ls:        $BSL_LS_CONFIG"
echo "  Конфиг bsl-analyzer:  $BSL_ANALYZER_CONFIG"
echo ""
echo "================================================================"

mkdir -p "$OUTPUT_DIR" "$RESULTS_DIR"
cd "$PROJECT_DIR"

# --- Функция одного прогона ---
# Args: tool_name, run_num, cmd...
run_one() {
    local tool="$1"
    local run="$2"
    shift 2
    local time_file="$RESULTS_DIR/${tool}_run${run}.time"

    # Очищаем output каждый раз
    rm -rf "$OUTPUT_DIR/${tool}-output"
    mkdir -p "$OUTPUT_DIR/${tool}-output"

    echo "  [${tool}] прогон $run/$RUNS..."
    /usr/bin/time -v -o "$time_file" "$@" > /dev/null 2>&1 || true

    local wall=$(grep "Elapsed (wall clock)" "$time_file" | sed 's/.*: //')
    local user=$(grep "^	User time" "$time_file" | awk '{print $NF}')
    local sys=$(grep "^	System time" "$time_file" | awk '{print $NF}')
    local rss_kb=$(grep "Maximum resident set size" "$time_file" | awk '{print $NF}')
    local cpu=$(grep "Percent of CPU" "$time_file" | awk '{print $NF}')

    # wall → seconds
    local wall_s
    if echo "$wall" | grep -qE "^[0-9]+:[0-9]+:[0-9]"; then
        wall_s=$(echo "$wall" | awk -F: '{print $1*3600 + $2*60 + $3}')
    elif echo "$wall" | grep -qE "^[0-9]+:[0-9]"; then
        wall_s=$(echo "$wall" | awk -F: '{print $1*60 + $2}')
    else
        wall_s="$wall"
    fi

    local rss_mb=$(echo "scale=1; $rss_kb / 1024" | bc)

    printf "    wall=%ss  user=%ss  sys=%ss  RSS=%sMB  CPU=%s\n" \
        "$wall_s" "$user" "$sys" "$rss_mb" "$cpu"

    # CSV для агрегации
    echo "$tool,$run,$wall_s,$user,$sys,$rss_mb,$cpu" >> "$RESULTS_DIR/all.csv"
}

# --- Медиана из CSV ---
median() {
    local tool="$1"
    local col="$2"  # 3=wall 4=user 5=sys 6=rss 7=cpu
    awk -F, -v t="$tool" -v c="$col" '$1==t {print $c}' "$RESULTS_DIR/all.csv" \
        | sort -n \
        | awk '
            { a[NR]=$1 }
            END {
                if (NR==0) { print "N/A"; exit }
                if (NR%2) print a[(NR+1)/2]
                else printf "%.2f", (a[NR/2]+a[NR/2+1])/2
            }
        '
}

# Очистка старых результатов
rm -f "$RESULTS_DIR/all.csv"
touch "$RESULTS_DIR/all.csv"

# --- Прогоны bsl-language-server ---
echo ""
echo "----------------------------------------------------------------"
echo "  bsl-language-server ($BSL_LS_VERSION, JVM $JVM_XMX)"
echo "----------------------------------------------------------------"
for i in $(seq 1 "$RUNS"); do
    run_one "bsl-ls" "$i" \
        "$BSL_LS" analyze \
        -c="$BSL_LS_CONFIG" \
        -s="$SOURCE_DIR" \
        -o="$OUTPUT_DIR/bsl-ls-output" \
        -r=json \
        -q
done

# --- Прогоны bsl-analyzer ---
echo ""
echo "----------------------------------------------------------------"
echo "  bsl-analyzer ($BSL_ANALYZER_VERSION)"
echo "----------------------------------------------------------------"
for i in $(seq 1 "$RUNS"); do
    run_one "bsl-analyzer" "$i" \
        "$BSL_ANALYZER" analyze \
        -c "$BSL_ANALYZER_CONFIG" \
        -s "$SOURCE_DIR" \
        -o "$OUTPUT_DIR/bsl-analyzer-output" \
        -r json \
        -q
done

# --- Итоги ---
echo ""
echo ""
echo "================================================================"
echo "  МЕДИАНЫ ($RUNS прогонов)"
echo "================================================================"
echo ""

LS_WALL=$(median "bsl-ls" 3)
LS_USER=$(median "bsl-ls" 4)
LS_SYS=$(median "bsl-ls" 5)
LS_RSS=$(median "bsl-ls" 6)

BA_WALL=$(median "bsl-analyzer" 3)
BA_USER=$(median "bsl-analyzer" 4)
BA_SYS=$(median "bsl-analyzer" 5)
BA_RSS=$(median "bsl-analyzer" 6)

SPEED=$(echo "scale=2; $LS_WALL / $BA_WALL" | bc 2>/dev/null || echo "?")
MEM=$(echo "scale=2; $LS_RSS / $BA_RSS" | bc 2>/dev/null || echo "?")
USER_R=$(echo "scale=2; $LS_USER / $BA_USER" | bc 2>/dev/null || echo "?")
FPS_LS=$(echo "scale=0; $BSL_COUNT / $LS_WALL" | bc 2>/dev/null || echo "?")
FPS_BA=$(echo "scale=0; $BSL_COUNT / $BA_WALL" | bc 2>/dev/null || echo "?")

printf "%-20s  %15s  %15s  %12s\n" "Метрика" "bsl-ls" "bsl-analyzer" "Разница"
printf "%-20s  %15s  %15s  %12s\n" "-------------------" "---------------" "---------------" "------------"
printf "%-20s  %14ss  %14ss  %10sx\n" "Wall time (median)"   "$LS_WALL" "$BA_WALL" "$SPEED"
printf "%-20s  %14ss  %14ss  %10sx\n" "User time (CPU)"      "$LS_USER" "$BA_USER" "$USER_R"
printf "%-20s  %14ss  %14ss\n"         "System time"         "$LS_SYS"  "$BA_SYS"
printf "%-20s  %13sMB  %13sMB  %10sx\n" "Peak RSS"           "$LS_RSS"  "$BA_RSS" "$MEM"
printf "%-20s  %15s  %15s\n"            "Files/sec"          "$FPS_LS"  "$FPS_BA"

echo ""
echo "================================================================"
echo "  ВСЕ ПРОГОНЫ (wall seconds)"
echo "================================================================"
echo ""
printf "%-15s" "tool"
for i in $(seq 1 "$RUNS"); do printf " run%-7s" "$i"; done
printf "\n"
for tool in bsl-ls bsl-analyzer; do
    printf "%-15s" "$tool"
    for i in $(seq 1 "$RUNS"); do
        v=$(awk -F, -v t="$tool" -v r="$i" '$1==t && $2==r {print $3}' "$RESULTS_DIR/all.csv")
        printf " %9s " "${v:-?}"
    done
    printf "\n"
done

echo ""
echo "Все замеры: $RESULTS_DIR/all.csv"
echo "Time logs:  $RESULTS_DIR/*.time"
echo ""
