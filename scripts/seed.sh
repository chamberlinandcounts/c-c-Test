#!/usr/bin/env bash
# Load the Canyon Oaks seed data into the db container.
# Expects ./seed/canyon_oaks.dump and ./seed/attachments/<table>/... (copied over
# with rsync; both are gitignored). Safe to re-run: public tables are replaced,
# issue tickets in schema `app` are kept.
set -euo pipefail
cd "$(dirname "$0")/.."
[ -f .env ] || { echo "Run scripts/setup-env.sh first"; exit 1; }
[ -f seed/canyon_oaks.dump ] || { echo "Missing seed/canyon_oaks.dump"; exit 1; }
set -a; . ./.env; set +a
DC="docker compose"
docker info >/dev/null 2>&1 || DC="sudo docker compose"

mkdir -p data/uploads data/tiles data/caddy
# API container runs as uid 10001
if [ "$(stat -c %u data/uploads)" != "10001" ]; then sudo chown 10001:10001 data/uploads; fi

$DC up -d db
echo "waiting for db..."
for i in $(seq 1 60); do $DC exec -T db pg_isready -U "$POSTGRES_USER" -d "$POSTGRES_DB" >/dev/null 2>&1 && break; sleep 2; done

$DC exec -T db sh -c "pg_restore -l /seed/canyon_oaks.dump | grep -v ' SCHEMA - public ' > /tmp/restore.list"
$DC exec -T db pg_restore -U "$POSTGRES_USER" -d "$POSTGRES_DB" --no-owner --no-privileges --clean --if-exists \
    -L /tmp/restore.list /seed/canyon_oaks.dump
$DC exec -T db psql -v ON_ERROR_STOP=1 -U "$POSTGRES_USER" -d "$POSTGRES_DB" <<'SQL'
-- spatial indexes for the (currently empty) layers ogr2ogr skipped
DO $$ DECLARE r record; BEGIN
  FOR r IN SELECT f_table_name t FROM geometry_columns g WHERE f_table_schema = 'public'
           AND NOT EXISTS (SELECT 1 FROM pg_indexes i WHERE i.tablename = g.f_table_name AND i.indexdef ILIKE '%gist%') LOOP
    EXECUTE format('CREATE INDEX ON public.%I USING gist (geom)', r.t);
  END LOOP; END $$;
ANALYZE;
SQL
$DC up -d --build
$DC restart api tiles   # api re-imports legacy reports; martin re-discovers tables
echo "Seed loaded. Table counts:"
$DC exec -T db psql -U "$POSTGRES_USER" -d "$POSTGRES_DB" -Atc \
  "SELECT format('%-26s %s', relname, n_live_tup) FROM pg_stat_user_tables WHERE schemaname IN ('public','app') ORDER BY schemaname, relname"
