#!/usr/bin/env bash

# Проверяет решение github-sync.sh о том, релизное ли синхронизируемое дерево, на
# боевом пути: клон, коммит, пуш, тег и gh release edit идут по-настоящему, но в
# локальный голый репозиторий и в заглушки gh/codex.

set -euo pipefail

SCRIPT_DIR=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
ROOT=$(cd "$SCRIPT_DIR/.." && pwd)
SYNC_SCRIPT=${SYNC_SCRIPT:-$ROOT/scripts/github-sync.sh}
TMP=$(mktemp -d)
if [[ "${KEEP_SYNC_TEST_TMP:-0}" == 1 ]]; then
  echo "keeping temp dir: $TMP" >&2
else
  trap 'rm -rf "$TMP"' EXIT
fi

BIN_DIR="$TMP/bin"
SRC="$TMP/src"
MIRROR="$TMP/mirror.git"
GH_LOG="$TMP/gh.log"
CODEX_LOG="$TMP/codex.log"
RELEASES="$TMP/releases.txt"
RUN_LOG="$TMP/run.log"
mkdir -p "$BIN_DIR"

FAILED=0
fail() {
  echo "  ПРОВАЛ: $*" >&2
  FAILED=1
}

assert_contains() {
  local file=$1 needle=$2 what=$3
  grep -qF -- "$needle" "$file" || fail "$what: в $(basename "$file") нет '$needle'"
}

assert_absent() {
  local file=$1 needle=$2 what=$3
  ! grep -qF -- "$needle" "$file" || fail "$what: в $(basename "$file") есть '$needle'"
}

assert_empty_file() {
  local file=$1 what=$2
  [[ ! -s "$file" ]] || fail "$what: $(basename "$file") не пуст"
}

assert_eq() {
  local got=$1 want=$2 what=$3
  [[ "$got" == "$want" ]] || fail "$what: получено '$got', ожидалось '$want'"
}

# gh: '--jq' намеренно не исполняется. Отбор базы notes живёт в bash github-sync.sh,
# и заглушка, отдающая уже отфильтрованное значение, проверяла бы саму себя.
cat > "$BIN_DIR/gh" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$*" >> "$GH_TEST_LOG"
if [[ "${1:-}" == release && "${2:-}" == list ]]; then
  cat "$GH_TEST_RELEASES"
fi
exit 0
EOF

cat > "$BIN_DIR/codex" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$*" >> "$CODEX_TEST_LOG"
out=""
prev=""
for arg in "$@"; do
  [[ "$prev" == "--output-last-message" ]] && out="$arg"
  prev="$arg"
done
cat > /dev/null
[[ -n "$out" ]] && printf 'Сводка изменений.\n' > "$out"
exit 0
EOF
chmod +x "$BIN_DIR/gh" "$BIN_DIR/codex"

mkdir -p "$SRC/scripts" "$SRC/crates/bsl-launcher"
cp "$SYNC_SCRIPT" "$SRC/scripts/github-sync.sh"
chmod +x "$SRC/scripts/github-sync.sh"

cat > "$SRC/Cargo.toml" <<'EOF'
[workspace.package]
version = "0.9.0"
EOF

echo 'INSTALL_SOURCE="gitlab"' > "$SRC/scripts/install.sh"
echo "\$InstallSource = 'gitlab'" > "$SRC/scripts/install.ps1"

cat > "$SRC/README.md" <<'EOF'
# fixture

<!-- INSTALL_URL:gitlab -->
gitlab
<!-- /INSTALL_URL -->

<!-- INSTALL_URL_WINDOWS:gitlab -->
gitlab
<!-- /INSTALL_URL_WINDOWS -->
EOF

echo '{"source":"github"}' > "$SRC/crates/bsl-launcher/release-source.github.json"
# Сам json из архива исключён, а cp в скрипте родительский каталог не создаёт:
# без второго отслеживаемого файла каталога в зеркале не будет и боевой путь упадёт.
echo '[package]' > "$SRC/crates/bsl-launcher/Cargo.toml"

git -C "$SRC" init -q -b develop
git -C "$SRC" config user.email fixture@example.test
git -C "$SRC" config user.name Fixture
git -C "$SRC" add -A
git -C "$SRC" commit -qm 'chore(release): v0.9.0'
git -C "$SRC" tag -a v0.9.0 -m 'Release v0.9.0'
TAG_COMMIT=$(git -C "$SRC" rev-parse HEAD)
echo 'после релиза' > "$SRC/after.md"
git -C "$SRC" add -A
git -C "$SRC" commit -qm 'fix: работа поверх релиза'
HEAD_COMMIT=$(git -C "$SRC" rev-parse HEAD)
HEAD_DESCRIBE=$(git -C "$SRC" describe --tags --always "$HEAD_COMMIT")

reset_mirror() {
  rm -rf "$MIRROR" "$TMP/seed"
  git init -q --bare -b develop "$MIRROR"
  git init -q -b develop "$TMP/seed"
  git -C "$TMP/seed" config user.email fixture@example.test
  git -C "$TMP/seed" config user.name Fixture
  git -C "$TMP/seed" commit -q --allow-empty -m 'init'
  git -C "$TMP/seed" push -q "$MIRROR" develop
}

run_sync() {
  local rev=$1
  shift
  reset_mirror
  : > "$GH_LOG"
  : > "$CODEX_LOG"
  git -C "$SRC" checkout -q "$rev"
  env -u GITHUB_SSH_KEY \
    PATH="$BIN_DIR:$PATH" \
    GH_TEST_LOG="$GH_LOG" \
    GH_TEST_RELEASES="$RELEASES" \
    CODEX_TEST_LOG="$CODEX_LOG" \
    GITHUB_SYNC_REPO="file://$MIRROR" \
    GITHUB_SYNC_REPO_SLUG="itrous/bsl-analyzer" \
    GIT_AUTHOR_NAME=Fixture GIT_AUTHOR_EMAIL=fixture@example.test \
    GIT_COMMITTER_NAME=Fixture GIT_COMMITTER_EMAIL=fixture@example.test \
    "$SRC/scripts/github-sync.sh" "$@" > "$RUN_LOG" 2>&1 \
    || { cat "$RUN_LOG" >&2; fail "прогон завершился ненулевым кодом"; }
}

mirror_subject() { git --git-dir="$MIRROR" log -1 --format=%s develop; }
mirror_body()    { git --git-dir="$MIRROR" log -1 --format=%b develop; }
mirror_tags()    { git --git-dir="$MIRROR" tag -l v0.9.0; }

# Список из двух релизов: с одним v0.9.0 верный отбор базы и пустая база дают
# одинаковый результат, и проверка ссылки была бы холостой.
printf 'v0.9.0\nv0.8.0\n' > "$RELEASES"

echo "A: дерево совпадает с тегом — синк релиза"
run_sync "$TAG_COMMIT"
assert_contains "$GH_LOG" 'release edit v0.9.0' 'I2 notes обновлены'
assert_eq "$(mirror_tags)" 'v0.9.0' 'I4 тег создан на зеркале'
assert_eq "$(mirror_subject)" 'Release v0.9.0' 'I4 субъект коммита'
mirror_body > "$TMP/body.txt"
assert_contains "$TMP/body.txt" 'compare/v0.8.0...v0.9.0' 'I7 база notes — предыдущий релиз'

echo "B: дерево ушло вперёд тега — синк состояния"
run_sync "$HEAD_COMMIT"
assert_absent "$GH_LOG" 'release edit' 'I1 notes не тронуты'
assert_eq "$(mirror_tags)" '' 'I3 тега на зеркале нет'
assert_eq "$(mirror_subject)" "Sync $HEAD_DESCRIBE" 'I8 субъект коммита'
assert_eq "$(mirror_body)" '' 'I9 тело коммита пусто'
assert_contains "$RUN_LOG" 'указывает на' 'диагностика называет несовпадение sha'
assert_absent "$GH_LOG" 'release list' 'gh не опрашивается: база notes синку состояния не нужна'
assert_empty_file "$CODEX_LOG" 'I10 codex не запускался'

echo "C: --force-release снимает классификацию для всех трёх действий"
run_sync "$HEAD_COMMIT" --force-release
assert_contains "$GH_LOG" 'release edit v0.9.0' 'I5 notes обновлены'
assert_eq "$(mirror_tags)" 'v0.9.0' 'I5 тег создан на зеркале'
assert_eq "$(mirror_subject)" 'Release v0.9.0' 'I5 субъект коммита'

echo "D: локального тега нет — синк состояния"
git -C "$SRC" tag -d v0.9.0 > /dev/null
run_sync "$TAG_COMMIT"
assert_absent "$GH_LOG" 'release edit' 'I6 notes не тронуты'
assert_eq "$(mirror_tags)" '' 'I6 тега на зеркале нет'
assert_absent "$RUN_LOG" 'указывает на' 'диагностика не выдумывает несовпадение sha'
git -C "$SRC" tag -a v0.9.0 -m 'Release v0.9.0' "$TAG_COMMIT"

if (( FAILED )); then
  echo "ТЕСТ ПРОВАЛЕН" >&2
  exit 1
fi
echo "все проверки пройдены"
