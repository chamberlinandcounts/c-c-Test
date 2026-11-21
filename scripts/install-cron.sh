#!/usr/bin/env bash
# Install the nightly backup cron entry for the current user (forge). Idempotent.
set -euo pipefail
LINE="30 2 * * * /srv/canyon-oaks/scripts/backup.sh >> /srv/canyon-oaks/backups/backup.log 2>&1"
existing=$(crontab -l 2>/dev/null | grep -v 'canyon-oaks/scripts/backup.sh' || true)
printf '%s\n%s\n' "$existing" "$LINE" | sed '/^$/d' | crontab -
crontab -l | grep backup.sh
