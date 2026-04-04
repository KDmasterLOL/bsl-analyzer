# Настройка сервера

Ниже — пример базового развёртывания PostgreSQL для централизованного поиска с
Vault-managed credentials.

## Предпосылки

- Ubuntu 24.04 или совместимая система;
- PostgreSQL 17;
- Vault с включённым `database` secrets engine;
- сетевой доступ от Vault к PostgreSQL.

## PostgreSQL

### Установка PostgreSQL 17 и `pgvector`

```bash
sudo apt install postgresql-17 postgresql-17-pgvector
```

Если в системе уже есть старая версия PostgreSQL:

```bash
sudo pg_dropcluster --stop <ver> main
sudo apt purge postgresql-<ver> postgresql-client-<ver>
sudo apt autoremove
```

### Базовая конфигурация

Убедитесь, что сервер слушает стандартный порт:

```bash
# /etc/postgresql/17/main/postgresql.conf
port = 5432
listen_addresses = '*'
```

```bash
sudo systemctl restart postgresql@17-main
```

### Создание базы, схемы и расширений

```bash
sudo -u postgres psql -p 5432 <<'SQL'
CREATE DATABASE bsl_search;
\c bsl_search
CREATE EXTENSION vector;
CREATE SCHEMA bsl_search;
SQL
```

### Роль администратора для Vault

```bash
sudo -u postgres psql -p 5432 -d bsl_search <<'SQL'
CREATE ROLE vault_pg_admin WITH LOGIN PASSWORD 'initial_password' CREATEROLE;
GRANT USAGE, CREATE ON SCHEMA bsl_search TO vault_pg_admin WITH GRANT OPTION;
GRANT ALL ON ALL TABLES IN SCHEMA bsl_search TO vault_pg_admin WITH GRANT OPTION;
ALTER DEFAULT PRIVILEGES IN SCHEMA bsl_search GRANT ALL ON TABLES TO vault_pg_admin;
SQL
```

После подключения Vault этот пароль обычно ротируется автоматически.

### `pg_hba.conf`

Добавьте правила доступа:

```text
host    bsl_search    vault_pg_admin    0.0.0.0/0    scram-sha-256
host    bsl_search    all               0.0.0.0/0    scram-sha-256
```

Для production ограничьте подсети вместо `0.0.0.0/0`.

```bash
sudo systemctl reload postgresql@17-main
```

## Vault

### Конфигурация подключения

```bash
vault write database/config/bsl-search \
  plugin_name=postgresql-database-plugin \
  connection_url="postgresql://{{username}}:{{password}}@<PG_HOST>:5432/bsl_search" \
  allowed_roles="bsl-search-reader,bsl-search-writer" \
  username="vault_pg_admin" \
  password="initial_password"
```

### Writer role

Используется CI и `sync-pg`:

```bash
vault write database/roles/bsl-search-writer \
  db_name=bsl-search \
  default_ttl=8h \
  max_ttl=24h \
  creation_statements="CREATE ROLE \"{{name}}\" WITH LOGIN PASSWORD '{{password}}' VALID UNTIL '{{expiration}}'; \
    GRANT USAGE, CREATE ON SCHEMA bsl_search TO \"{{name}}\"; \
    GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA bsl_search TO \"{{name}}\"; \
    ALTER DEFAULT PRIVILEGES FOR ROLE vault_pg_admin IN SCHEMA bsl_search \
      GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO \"{{name}}\";" \
  revocation_statements="SET ROLE vault_pg_admin; \
    ALTER DEFAULT PRIVILEGES IN SCHEMA bsl_search REVOKE ALL ON TABLES FROM \"{{name}}\"; \
    REVOKE ALL PRIVILEGES ON ALL TABLES IN SCHEMA bsl_search FROM \"{{name}}\"; \
    REVOKE ALL PRIVILEGES ON SCHEMA bsl_search FROM \"{{name}}\"; \
    RESET ROLE; \
    DROP ROLE IF EXISTS \"{{name}}\";"
```

### Reader role

Используется developer runtime:

```bash
vault write database/roles/bsl-search-reader \
  db_name=bsl-search \
  default_ttl=8h \
  max_ttl=24h \
  creation_statements="CREATE ROLE \"{{name}}\" WITH LOGIN PASSWORD '{{password}}' VALID UNTIL '{{expiration}}'; \
    GRANT USAGE ON SCHEMA bsl_search TO \"{{name}}\"; \
    GRANT SELECT ON ALL TABLES IN SCHEMA bsl_search TO \"{{name}}\";" \
  revocation_statements="SET ROLE vault_pg_admin; \
    REVOKE ALL PRIVILEGES ON ALL TABLES IN SCHEMA bsl_search FROM \"{{name}}\"; \
    REVOKE ALL PRIVILEGES ON SCHEMA bsl_search FROM \"{{name}}\"; \
    RESET ROLE; \
    DROP ROLE IF EXISTS \"{{name}}\";"
```

## Почему в revoke используется `SET ROLE`

`vault_pg_admin` не является `SUPERUSER`, поэтому revoke должен выполняться от
того grantor'а, который выдавал права. Иначе `ALTER DEFAULT PRIVILEGES ... REVOKE`
будет падать по permission error.

## Проверка

```bash
# 1. Получить временные креды
vault read database/creds/bsl-search-writer

# 2. Подключиться и проверить права
PGPASSWORD=<password> psql -h <PG_HOST> -U <username> -d bsl_search \
  -c "CREATE TABLE bsl_search.test_tbl (id int); DROP TABLE bsl_search.test_tbl;"

# 3. Отозвать lease
vault lease revoke <lease_id>
```
