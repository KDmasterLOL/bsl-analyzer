#!/usr/bin/env bash
# Установка git hooks для bsl-analyzer

set -e

# Цвета
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
GIT_HOOKS_DIR="$PROJECT_ROOT/.git/hooks"

echo -e "${YELLOW}Setting up git hooks...${NC}\n"

# Проверяем, что мы в git репозитории
if [ ! -d "$PROJECT_ROOT/.git" ]; then
    echo "Error: Not a git repository!"
    exit 1
fi

# Создаём директорию hooks если её нет
mkdir -p "$GIT_HOOKS_DIR"

# Устанавливаем pre-commit hook
echo -e "${YELLOW}Installing pre-commit hook...${NC}"
cp "$SCRIPT_DIR/pre-commit" "$GIT_HOOKS_DIR/pre-commit"
chmod +x "$GIT_HOOKS_DIR/pre-commit"
echo -e "${GREEN}✓ Pre-commit hook installed${NC}\n"

# Информация
echo -e "${GREEN}✓ Git hooks installed successfully!${NC}\n"
echo "Pre-commit hook will now run before each commit:"
echo "  - cargo fmt --all (auto-format and stage changes)"
echo "  - cargo clippy --all-targets --all-features -- -D warnings"
echo ""
echo "To skip hooks temporarily, use: git commit --no-verify"
echo ""
