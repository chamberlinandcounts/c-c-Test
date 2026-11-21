#!/usr/bin/env bash
# Install the nightly backup cron entry for the current user (forge). Idempotent.
set -euo pipefail
LINE="30 2 * * * /srv/canyon-oaks/scripts/backup.sh >> /srv/canyon-oaks/backups/backup.log 2>&1"
( crontab -l 2>/dev/null | grep -v 'canyon-oaks/scripts/backup.sh' ; echo "$LINE" ) | crontab -
crontab -l | grep backup.sh
