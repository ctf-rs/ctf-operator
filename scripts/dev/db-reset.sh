#!/usr/bin/env bash
# Reset the database to a clean state (drops all app tables).
#
# Usage: ./scripts/dev/db-reset.sh

set -euo pipefail

CONTAINER="app-postgres-1"
DB_USER="ctf"
DB_NAME="ctf_app"

echo "=== Dropping all app tables ==="
docker exec -i "${CONTAINER}" psql -U "${DB_USER}" -d "${DB_NAME}" <<'SQL'
DROP TABLE IF EXISTS scoreboard_cache CASCADE;
DROP TABLE IF EXISTS solved_challenges CASCADE;
DROP TABLE IF EXISTS flag_submissions CASCADE;
DROP TABLE IF EXISTS instance_flags CASCADE;
DROP TABLE IF EXISTS instance_actions CASCADE;
DROP TABLE IF EXISTS downloads CASCADE;
DROP TABLE IF EXISTS sessions CASCADE;
DROP TABLE IF EXISTS users CASCADE;
SQL

echo "=== Database reset. Tables will be recreated on next app start. ==="
