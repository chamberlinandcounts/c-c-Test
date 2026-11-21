#!/usr/bin/env bash
# Create .env from .env.example with freshly generated secrets (run once on the server).
set -euo pipefail
cd "$(dirname "$0")/.."
if [ -f .env ]; then echo ".env already exists; leaving it alone."; exit 0; fi
umask 077
pw=$(openssl rand -hex 24)
tok=$(openssl rand -hex 24)
sed -e "s|^POSTGRES_PASSWORD=.*|POSTGRES_PASSWORD=${pw}|" -e "s|^ADMIN_TOKEN=.*|ADMIN_TOKEN=${tok}|" .env.example > .env
chmod 600 .env
mkdir -p data/postgres data/uploads data/tiles data/caddy backups
echo "Wrote .env (mode 600). Staff token for the Issue Tracker is ADMIN_TOKEN in .env."
