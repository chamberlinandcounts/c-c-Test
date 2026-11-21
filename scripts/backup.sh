#!/usr/bin/env bash
# Nightly backup: pg_dump of the whole database + copy of uploaded issue photos.
# Cron (user forge): 30 2 * * * /srv/canyon-oaks/scripts/backup.sh >> /srv/canyon-oaks/backups/backup.log 2>&1
set -euo pipefail
cd "$(dirname "$0")/.."
DEST=${BACKUP_DIR:-/srv/canyon-oaks/backups}
KEEP_DAYS=${KEEP_DAYS:-14}
set -a; . ./.env; set +a
DC="docker compose"
docker info >/dev/null 2>&1 || DC="sudo -n docker compose"
mkdir -p "$DEST/db" "$DEST/uploads"
umask 077
ts=$(date +%Y%m%d_%H%M%S)
$DC exec -T db pg_dump -U "$POSTGRES_USER" -d "$POSTGRES_DB" -Fc > "$DEST/db/canyon_oaks_$ts.dump.part"
mv "$DEST/db/canyon_oaks_$ts.dump.part" "$DEST/db/canyon_oaks_$ts.dump"
rsync -a data/uploads/ "$DEST/uploads/" 2>/dev/null || sudo -n rsync -a data/uploads/ "$DEST/uploads/"
find "$DEST/db" -name 'canyon_oaks_*.dump' -mtime +"$KEEP_DAYS" -delete
echo "$(date -Is) backup ok: $DEST/db/canyon_oaks_$ts.dump ($(du -h "$DEST/db/canyon_oaks_$ts.dump" | cut -f1))"
