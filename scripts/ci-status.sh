#!/usr/bin/env bash
# GitLab CI Status Checker для bsl-analyzer
# Использование: ./scripts/ci-status.sh [pipeline_id]

set -euo pipefail

# Цвета для вывода
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Конфигурация
GITLAB_URL="https://gitlab.runsystems.ru"
PROJECT_PATH="proit%2Fbsl-analyzer"

# Получаем токен из git config
get_token() {
    local token
    token=$(git config --global gitlab.token 2>/dev/null || echo "")
    if [[ -z "$token" ]]; then
        echo -e "${RED}Error: GitLab token not found${NC}" >&2
        echo "Run: git config --global gitlab.token YOUR_TOKEN" >&2
        exit 1
    fi
    echo "$token"
}

# Форматирование статуса с цветом
format_status() {
    local status=$1
    case "$status" in
        success)
            echo -e "${GREEN}✓ $status${NC}"
            ;;
        failed)
            echo -e "${RED}✗ $status${NC}"
            ;;
        running)
            echo -e "${BLUE}⟳ $status${NC}"
            ;;
        pending)
            echo -e "${YELLOW}⧖ $status${NC}"
            ;;
        skipped)
            echo -e "${YELLOW}⊝ $status${NC}"
            ;;
        canceled)
            echo -e "${YELLOW}⊗ $status${NC}"
            ;;
        *)
            echo "$status"
            ;;
    esac
}

# Получить информацию о последнем pipeline
get_latest_pipeline() {
    local token=$1
    curl -s --header "PRIVATE-TOKEN: $token" \
        "$GITLAB_URL/api/v4/projects/$PROJECT_PATH/pipelines?per_page=1"
}

# Получить информацию о конкретном pipeline
get_pipeline() {
    local token=$1
    local pipeline_id=$2
    curl -s --header "PRIVATE-TOKEN: $token" \
        "$GITLAB_URL/api/v4/projects/$PROJECT_PATH/pipelines/$pipeline_id"
}

# Получить jobs для pipeline
get_pipeline_jobs() {
    local token=$1
    local pipeline_id=$2
    curl -s --header "PRIVATE-TOKEN: $token" \
        "$GITLAB_URL/api/v4/projects/$PROJECT_PATH/pipelines/$pipeline_id/jobs"
}

# Показать краткую информацию о pipeline
show_pipeline_summary() {
    local pipeline_json=$1

    local id iid status ref sha created_at web_url
    id=$(echo "$pipeline_json" | jq -r '.id')
    iid=$(echo "$pipeline_json" | jq -r '.iid')
    status=$(echo "$pipeline_json" | jq -r '.status')
    ref=$(echo "$pipeline_json" | jq -r '.ref')
    sha=$(echo "$pipeline_json" | jq -r '.sha[0:8]')
    created_at=$(echo "$pipeline_json" | jq -r '.created_at')
    web_url=$(echo "$pipeline_json" | jq -r '.web_url')

    echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
    echo -e "${BLUE}Pipeline #$iid${NC} (ID: $id)"
    echo -e "Status:  $(format_status "$status")"
    echo -e "Branch:  $ref"
    echo -e "Commit:  $sha"
    echo -e "Created: $created_at"
    echo -e "URL:     $web_url"
    echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
}

# Показать детальную информацию о jobs
show_jobs_detail() {
    local jobs_json=$1

    echo -e "\n${BLUE}Jobs:${NC}"
    printf "%-15s %-25s %-12s %-10s\n" "STAGE" "NAME" "STATUS" "DURATION"
    echo "────────────────────────────────────────────────────────────────"

    echo "$jobs_json" | jq -r '.[] |
        [.stage, .name, .status, (.duration // 0)] |
        @tsv' | while IFS=$'\t' read -r stage name status duration; do

        # Форматируем длительность
        if [[ "$duration" == "0" ]] || [[ "$duration" == "null" ]]; then
            duration_str="-"
        else
            duration_str="${duration}s"
        fi

        # Форматируем статус
        status_colored=$(format_status "$status")

        printf "%-15s %-25s %-22s %-10s\n" \
            "$stage" "$name" "$status_colored" "$duration_str"
    done
}

# Показать логи упавшего job
show_failed_job_log() {
    local token=$1
    local job_id=$2
    local job_name=$3

    echo -e "\n${RED}━━━ Failed Job: $job_name (ID: $job_id) ━━━${NC}\n"

    local trace
    trace=$(curl -s --header "PRIVATE-TOKEN: $token" \
        "$GITLAB_URL/api/v4/projects/$PROJECT_PATH/jobs/$job_id/trace")

    # Убираем ANSI escape коды и показываем последние 50 строк
    echo "$trace" | sed 's/\x1b\[[0-9;]*m//g' | tail -50
}

# Основная функция
main() {
    local pipeline_id=${1:-}

    # Проверяем наличие jq
    if ! command -v jq &> /dev/null; then
        echo -e "${RED}Error: jq is required but not installed${NC}" >&2
        echo "Install: brew install jq (macOS) or apt install jq (Linux)" >&2
        exit 1
    fi

    local token
    token=$(get_token)

    local pipeline_json
    if [[ -z "$pipeline_id" ]]; then
        # Получаем последний pipeline
        pipeline_json=$(get_latest_pipeline "$token" | jq '.[0]')
        if [[ "$pipeline_json" == "null" ]]; then
            echo -e "${RED}No pipelines found${NC}" >&2
            exit 1
        fi
    else
        # Получаем конкретный pipeline
        pipeline_json=$(get_pipeline "$token" "$pipeline_id")
        if [[ $(echo "$pipeline_json" | jq -r '.id') == "null" ]]; then
            echo -e "${RED}Pipeline #$pipeline_id not found${NC}" >&2
            exit 1
        fi
    fi

    # Показываем информацию о pipeline
    show_pipeline_summary "$pipeline_json"

    # Получаем и показываем jobs
    local id
    id=$(echo "$pipeline_json" | jq -r '.id')
    local jobs_json
    jobs_json=$(get_pipeline_jobs "$token" "$id")

    show_jobs_detail "$jobs_json"

    # Если есть упавшие jobs, показываем их логи
    local failed_jobs
    failed_jobs=$(echo "$jobs_json" | jq -r '.[] | select(.status == "failed") | .id + ":" + .name')

    if [[ -n "$failed_jobs" ]]; then
        echo -e "\n${YELLOW}Showing logs for failed jobs...${NC}"
        while IFS=: read -r job_id job_name; do
            show_failed_job_log "$token" "$job_id" "$job_name"
        done <<< "$failed_jobs"
    fi

    echo ""
}

# Запуск
main "$@"
