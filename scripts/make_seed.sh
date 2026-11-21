#!/usr/bin/env bash
# Run on the machine that holds the downloaded AGO export (the build box).
# Produces ./seed/canyon_oaks.dump and ./seed/attachments/<table>/<file>
# from the local PostGIS db `canyon_oaks` and /workspace/canyon-oaks/attachments.
# Output is gitignored; copy ./seed/ to the server with rsync.
set -euo pipefail
SRC_DB=${SRC_DB:-canyon_oaks}
SRC_ATT=${SRC_ATT:-/workspace/canyon-oaks/attachments}
cd "$(dirname "$0")/.."
mkdir -p seed/attachments
declare -A MAP=(
  [signs_practice_with_photos__signs]=signs
  [fire_hydrants_practice__fire_hydrants]=fire_hydrants
  [irrigation_practice__irrigation]=irrigation
  [canyon_oaks_report_an_issue_public_view__survey]=issue_reports
)
for src in "${!MAP[@]}"; do
  dst=seed/attachments/${MAP[$src]}; mkdir -p "$dst"
  find "$SRC_ATT/$src" -maxdepth 1 -type f ! -name '*.csv' -exec cp -l -n {} "$dst/" \; 2>/dev/null || \
  find "$SRC_ATT/$src" -maxdepth 1 -type f ! -name '*.csv' -exec cp -n {} "$dst/" \;
done
# Relative attachment paths; the inspection view shares the Signs photos.
psql -q -d "$SRC_DB" <<'SQL'
UPDATE attachments SET file_path = regexp_replace(file_path, '^.*/attachments/[^/]+/', table_name || '/') WHERE table_name <> 'signs_inspection_view';
UPDATE attachments SET file_path = regexp_replace(file_path, '^.*/attachments/[^/]+/', 'signs/') WHERE table_name = 'signs_inspection_view';
SQL
pg_dump -d "$SRC_DB" -n public -Fc -O -x -f seed/canyon_oaks.dump
ls -la seed; du -sh seed/attachments
