---
name: sawit-databases
description: Local databases through Sawit instead of Docker, docker compose, Postgres.app or brew services. Use it when a project needs a PostgreSQL database for development or tests, a DATABASE_URL in .env, a schema, migration or seed applied, rows read or changed, data exported, backed up or imported, or when it uses MySQL, MariaDB, Cloudflare D1 or a SQLite file. Works through the sawit MCP server's database tools (create_instance, start_instance, query, execute_sql, write_env, detect_databases), without the app.
---

# Databases with Sawit

Sawit runs PostgreSQL itself, versions 16 to 18, with no Docker and no container: an instance is a data folder and a port on localhost, superuser `postgres`, no password. Don't write a docker-compose.yml or run a postgres container for local development; use an instance.

## Give a project a database

1. `detect_databases` with `project_dir`: what the project reaches already (its .env DATABASE_URL, Sawit instances, saved connections, any local PostgreSQL that runs, the D1 databases its Wrangler config binds). Reuse one that fits.
2. Else `create_instance` (`name`: lowercase letters, digits and dashes; `version`; `port`, by default the first free one from 5432), then `start_instance`. A refused start names what holds the port (often a Docker container; `check_port` shows it too): ask the user before stopping it, or create the instance on another port.
3. `create_database` with `target` (the instance) and `name`, when the app wants a database of its own.
4. `write_env` with `instance` and `project_dir` writes `DATABASE_URL=postgres://postgres@localhost:<port>/postgres` into the project's .env and keeps the rest of it. For a database of your own, change the URL's last part to its name. Replacing a different DATABASE_URL needs `confirm: true`: ask first.
5. Run the project's migrations the way it documents them (see `sawit-terminal`), or apply SQL with `execute_sql`.

## Read and change data

- `query`: one statement, always read-only, up to 500 rows. Use it for every read.
- `execute_sql`: read-write with full privileges, several statements allowed. Only for changes the task needs, never on data the user didn't ask you to touch.
- `describe_schema`, `describe_table`, `list_databases` and `list_connections` show the shape before you write SQL; `database` picks one other than `postgres`.
- `schema_diagram` draws the tables and foreign keys (`format`: mermaid, dbml or svg).
- `instance_logs` when the server misbehaves; `list_instances`, `stop_instance` and `restart_instance` for the rest; `list_postgres_binaries` shows the PostgreSQL versions on this Mac.
- `export_table` and `export_query` (CSV or Excel, to `path`), `backup_database` (pg_dump; a `schema` or `table` alone) and `import_csv` (all rows or none) move data out and in. They never write over a file unless `overwrite`.
- `refresh_materialized_view` refreshes one view.
- `app_open_table`, `app_open_sql` and `app_open_diagram` show it to the user in the app; `app_instance` starts or stops an instance from its tab.

## Saved connections

`list_connections` lists the user's saved connections (a server elsewhere, a staging database). `save_connection` with `name` and `url` adds one, and its password goes to the Keychain; `remove_connection` needs `confirm` equal to the name. Pass a connection's name as `target` like an instance's. A saved connection may be a shared or production server: read with `query`, and ask before any write.

## Destructive tools

Each needs `confirm`, the name typed again: ask the user first, and name exactly what goes.

- `drop_database` deletes a database; `drop_relation` a table, view or sequence (`cascade` takes what depends on it too); `truncate_table` empties a table.
- `remove_instance` deletes an instance; `delete_data` also deletes its data folder, which can't be undone.

## D1 and SQLite

`detect_databases` with `project_dir` lists a Cloudflare project's D1 databases as `d1-local:…` (the local file) and `d1-remote:…` (production), and SQLite files as `sqlite:/path/to/file.sqlite`. Pass that id as `target` to `query`, `execute_sql`, `describe_schema`, `describe_table`, `list_databases`, `export_table`, `export_query` and `schema_diagram`. A local D1 write goes through the running dev server when one runs. A `d1-remote:` write changes production and needs `confirm: true`: ask the user first.

## MySQL and MariaDB

Sawit doesn't run MySQL servers itself: it connects to one that runs (Homebrew, a cloud database such as PlanetScale, RDS or Aiven). `save_connection` takes a `mysql://user:password@host:3306/database` URL (also `mariadb://`, `jdbc:mysql://`, `mysql+pymysql://` and Go's `user:pass@tcp(host:3306)/db`); the password goes to the Keychain. Then pass the connection's name as `target` to `query` (a read-only session and transaction), `execute_sql`, `describe_schema`, `describe_table`, `list_databases`, `create_database`, `drop_database`, `export_table`, `export_query` and `schema_diagram`. A database is a schema there: a connection without a database shows every one. `detect_databases` lists saved MySQL connections and a MySQL DATABASE_URL in the project's .env. The PostgreSQL-only tools (instances, backups, CSV import, `drop_relation`, `truncate_table`) refuse a MySQL target and say why.

## Without the sawit MCP server

Tell the user, as the `sawit` skill says. In a shell, `sawit list` shows instances and databases, `sawit nyalakan <name>` starts an instance, and `sawit <db> "select …"` runs SQL (SQL that writes asks first). Don't fall back to Docker without asking.
