#!/bin/bash

# Бенчмарк: время выполнения и пиковая память
# Использует /usr/bin/time для точных измерений через getrusage()

if [ $# -eq 0 ]; then
  echo "Использование: $0 <команда> [аргументы]"
  echo "Пример: $0 cargo run --release -- analyze ."
  exit 1
fi

echo "📊 Запуск: $*"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""

# Запускаем через /usr/bin/time -l (macOS) для точных метрик
output=$(/usr/bin/time -l "$@" 2>&1)
exit_code=$?

# Извлекаем метрики из вывода time
real_time=$(echo "$output" | grep -E "^\s+[0-9]+[,\.][0-9]+ real" | awk '{print $1}')
user_time=$(echo "$output" | grep -E "^\s+[0-9]+[,\.][0-9]+ real" | awk '{print $3}')
sys_time=$(echo "$output" | grep -E "^\s+[0-9]+[,\.][0-9]+ real" | awk '{print $5}')
max_rss=$(echo "$output" | grep "maximum resident set size" | awk '{print $1}')
peak_footprint=$(echo "$output" | grep "peak memory footprint" | awk '{print $1}')

# Конвертируем в MB
if [ -n "$max_rss" ]; then
  max_rss_mb=$(echo "scale=1; $max_rss / 1024 / 1024" | bc)
else
  max_rss_mb="N/A"
fi

if [ -n "$peak_footprint" ]; then
  peak_mb=$(echo "scale=1; $peak_footprint / 1024 / 1024" | bc)
else
  peak_mb="N/A"
fi

echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""
echo "✅ РЕЗУЛЬТАТЫ:"
echo "  ⏱️  Время (real/user/sys): ${real_time}s / ${user_time}s / ${sys_time}s"
echo "  💾 Пиковая память (RSS):   ${max_rss_mb} MB"
echo "  💾 Peak memory footprint:  ${peak_mb} MB"
echo "  📌 Exit code: $exit_code"
echo ""

exit $exit_code
