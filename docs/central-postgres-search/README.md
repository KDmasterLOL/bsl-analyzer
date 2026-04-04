# Централизованный поиск в PostgreSQL

Этот каталог — навигация по текущей реализации centralized search в
`bsl-analyzer`. Здесь собраны только документы про уже используемую архитектуру:
roadmap- и planning-файлы из репозитория удалены.

## Базовая модель

Подсистема строится вокруг двух источников:

- **shared baseline** — опубликованный snapshot в PostgreSQL;
- **local overlay** — локальные добавления, изменения и удаления рабочей копии.

Runtime работает с логическим представлением:

```text
resolved view = selected baseline + local overlay
```

## С чего читать

Если нужно быстро сориентироваться:

- сначала `01-vision.md` и `02-adr-postgres-first.md`;
- затем один из runtime-документов: `05-runtime-reference.md` или `06-runtime-workspace.md`;
- после этого `07-overlay-merge.md`, если важны правила объединения baseline и локальных файлов.

Если нужна эксплуатация или внедрение:

- `08-publishing.md` — публикация snapshot'ов;
- `09-operations.md` — retention, GC, наблюдаемость;
- `12-server-setup.md` — пример развёртывания PostgreSQL + Vault;
- `13-cli-commands.md` — CLI-команды для baseline в PostgreSQL.

## Полная карта раздела

- `01-vision.md` — зачем проекту централизованный поиск
- `02-adr-postgres-first.md` — почему shared runtime опирается на PostgreSQL
- `03-domain-model.md` — основные сущности: corpus, snapshot, file object, overlay
- `04-storage-schema.md` — схема хранения в PostgreSQL
- `05-runtime-reference.md` — runtime для профиля `reference`
- `06-runtime-workspace.md` — runtime для профиля `workspace`
- `07-overlay-merge.md` — правила объединения baseline и локального overlay
- `08-publishing.md` — публикация snapshot'ов и инварианты publish pipeline
- `09-operations.md` — эксплуатация, retention, GC и наблюдаемость
- `12-server-setup.md` — пример развёртывания PostgreSQL + Vault
- `13-cli-commands.md` — команды `search baseline *-pg`
