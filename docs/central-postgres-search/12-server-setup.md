# Server Setup

Step-by-step guide for deploying a PostgreSQL server for central code search
with Vault-managed dynamic credentials.

## Prerequisites

- Ubuntu 24.04 (or compatible)
- PostgreSQL 17
- HashiCorp Vault with `database` secrets engine enabled
- Network access from Vault to PostgreSQL

## PostgreSQL

### Install PostgreSQL 17 and pgvector

```bash
sudo apt install postgresql-17 postgresql-17-pgvector
```

If a previous PostgreSQL version is installed, remove it:

```bash
sudo pg_dropcluster --stop <ver> main
sudo apt purge postgresql-<ver> postgresql-client-<ver>
sudo apt autoremove
```

Ensure PostgreSQL 17 is on the standard port:

```bash
# /etc/postgresql/17/main/postgresql.conf
port = 5432
listen_addresses = '*'
```

```bash
sudo systemctl restart postgresql@17-main
```

### Create database, schema, and extensions

```bash
sudo -u postgres psql -p 5432 <<'SQL'
CREATE DATABASE bsl_search;
\c bsl_search
CREATE EXTENSION vector;
CREATE SCHEMA bsl_search;
SQL
```

### Create Vault admin role

```bash
sudo -u postgres psql -p 5432 -d bsl_search <<'SQL'
CREATE ROLE vault_pg_admin WITH LOGIN PASSWORD 'initial_password' CREATEROLE;
GRANT USAGE, CREATE ON SCHEMA bsl_search TO vault_pg_admin WITH GRANT OPTION;
GRANT ALL ON ALL TABLES IN SCHEMA bsl_search TO vault_pg_admin WITH GRANT OPTION;
ALTER DEFAULT PRIVILEGES IN SCHEMA bsl_search GRANT ALL ON TABLES TO vault_pg_admin;
SQL
```

> After Vault connection is created, Vault rotates this password automatically.
> The initial password stops working — this is expected.

### Configure pg_hba.conf

Add to `/etc/postgresql/17/main/pg_hba.conf`:

```
host    bsl_search    vault_pg_admin    0.0.0.0/0    scram-sha-256
host    bsl_search    all               0.0.0.0/0    scram-sha-256
```

For production, restrict `0.0.0.0/0` to specific networks:

```
host    bsl_search    vault_pg_admin    10.173.42.0/24    scram-sha-256
host    bsl_search    all               10.173.42.0/24    scram-sha-256
```

```bash
sudo systemctl reload postgresql@17-main
```

## Vault

### Connection config

```bash
vault write database/config/bsl-search \
  plugin_name=postgresql-database-plugin \
  connection_url="postgresql://{{username}}:{{password}}@<PG_HOST>:5432/bsl_search" \
  allowed_roles="bsl-search-reader,bsl-search-writer" \
  username="vault_pg_admin" \
  password="initial_password"
```

### Writer role

Used by CI pipelines and `bsl-analyzer sync-pg` for publishing.

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

Used by developer runtimes (MCP server) for search queries.

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

### Vault policy for credential helper

Minimal policy for `rtools credential-helper bsl-search`:

```hcl
path "database/creds/bsl-search-writer" {
  capabilities = ["read"]
}
path "database/creds/bsl-search-reader" {
  capabilities = ["read"]
}
```

## Revocation statements — why SET ROLE

`vault_pg_admin` is CREATEROLE but not SUPERUSER. Dynamic role grants are
issued by `vault_pg_admin` (it's the connection user). When Vault revokes a
lease, it needs to:

1. Revoke default privileges — requires being the grantor (`vault_pg_admin`)
2. Revoke schema/table privileges — requires being the grantor
3. Drop the role — requires no remaining dependencies

Without `SET ROLE vault_pg_admin` in revocation_statements, Vault executes
revocation as the connection user directly, but PG checks the grantor for
`ALTER DEFAULT PRIVILEGES ... REVOKE` and fails with permission errors.

`REASSIGN OWNED BY` and `DROP OWNED BY` also fail because they require
SUPERUSER or membership in the target role — which vault_pg_admin does not
have for the dynamic roles it created.

The solution: `SET ROLE vault_pg_admin` → revoke all grants → `RESET ROLE` →
`DROP ROLE`.

## Verification

Full lifecycle test:

```bash
# 1. Generate credentials
vault read database/creds/bsl-search-writer
# → username, password, lease_id

# 2. Connect
PGPASSWORD=<password> psql -h <PG_HOST> -U <username> -d bsl_search \
  -c "CREATE TABLE bsl_search.test_tbl (id int); DROP TABLE bsl_search.test_tbl;"

# 3. Revoke
vault lease revoke <lease_id>

# 4. Verify role is gone
psql -U postgres -d bsl_search \
  -c "SELECT rolname FROM pg_roles WHERE rolname LIKE 'v-root-%';"
# → 0 rows
```

If step 4 shows remaining roles, check that revocation_statements include
`SET ROLE vault_pg_admin` and the correct `ALTER DEFAULT PRIVILEGES` revoke.
