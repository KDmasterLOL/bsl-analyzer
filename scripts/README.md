# Scripts

Вспомогательные скрипты для разработки bsl-analyzer.

## setup-hooks.sh

Установка git pre-commit hooks.

```bash
./scripts/setup-hooks.sh
```

Устанавливает pre-commit hook в `.git/hooks/`, который автоматически запускает перед каждым коммитом:

- `cargo fmt --all -- --check`
- `cargo clippy --all-targets --all-features -- -D warnings`

## release.sh

Локальная сборка релиза.

```bash
./scripts/release.sh 0.1.37
```

- Собирает `bsl-analyzer` в release mode
- Определяет платформу (linux/darwin/windows, amd64/arm64)
- Вычисляет SHA256 checksum
