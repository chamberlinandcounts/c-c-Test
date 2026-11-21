# Canyon Oaks open-source GIS stack

Self-hosted replacement for the Canyon Oaks ArcGIS Online setup (hosted feature
layers, web map, Instant App, Asset Dashboard, Issue Tracker and the Survey123
"Report an Issue" form): **PostGIS + Rust (Martin tile server, Axum API) + MapLibre GL JS**,
run with Docker Compose on the mini PC behind the Cloudflare tunnel.

Built by Forge for Chamberlin & Counts.

## Architecture

```
 browser ──► Cloudflare tunnel ──► 127.0.0.1:8080 ──► web (Caddy)
                                                      ├─ /            static site (MapLibre pages)
                                                      ├─ /api/*   ──► api   (Rust: Axum + SQLx)  ──┐
                                                      └─ /tiles/* ──► tiles (Martin)            ──┤
                                                                     └─ ortho.mbtiles (raster)    ▼
                                                                                         db (PostGIS 17)
```

| Service | Image | Role |
|---|---|---|
| `db` | `postgis/postgis:17-3.5` | All Canyon Oaks layers (schema `public`, same table names as the AGO export) and issue tickets (schema `app`). Data in `./data/postgres`. No published port. |
| `tiles` | `ghcr.io/maplibre/martin` | Vector tiles for every spatial table in `public` + the 2024 ortho from `./data/tiles/ortho.mbtiles`. The `app` schema (reporter names/emails) is never published. |
| `api` | built from `./api` | Layers, features, attachments (photos), parcel search, dashboard stats, issue reporting. |
| `web` | `caddy:2-alpine` | Serves `./web/site` and reverse-proxies `/api` and `/tiles`. The only published port: `127.0.0.1:${WEB_PORT:-8080}`. |

Pages: `/` Community Map, `/assets` Asset Dashboard, `/issues` Issue Tracker, `/report` Report an Issue (mobile-first).
Layer styling, pop-up fields and domains in `web/site/config/layers.json` are generated from the saved AGO web map
by `scripts/build_web_config.py`.

### API

| Method & path | Notes |
|---|---|
| `GET /api/health` | `{"status":"ok"}` when the DB answers |
| `GET /api/layers` | tables with geometry type, feature and attachment counts |
| `GET /api/features/{layer}?bbox=minLon,minLat,maxLon,maxLat&fid=&limit=` | GeoJSON (fid = `ogc_fid`, also the vector-tile feature id) |
| `GET /api/search?q=` | parcels by address or APN |
| `GET /api/stats` | Asset Dashboard numbers and charts |
| `GET /api/attachments/{layer}/{objectid}` / `.../{attachment_id}` | photo list / file |
| `GET /api/issues/options` | categories, issue types (verbatim from the Survey123 XLSForm), statuses |
| `POST /api/issues` | multipart: `name`, `street_address`, `email`, `hoa_account_number`, `issue_category`, `issue_type`, `describe_the_issue`, `additional_notes`, `lat`, `lon`, up to 3 `photos`. Returns `{ticket: "CO-YYMMDD-NNNN"}`. Rate limited per IP (`RATE_LIMIT_PER_HOUR`). |
| `GET /api/issues?status=&category=` | public fields; reporter name/address/email only with the staff token |
| `GET /api/issues/{ticket}` | one ticket |
| `PATCH /api/issues/{ticket}` | staff: `{status: New|In Progress|Closed, assigned_to, notes}` |
| `DELETE /api/issues/{ticket}` | staff |
| `GET /api/issues/{ticket}/photos/{id}` | issue photo |

Staff endpoints need `Authorization: Bearer $ADMIN_TOKEN` (the Issue Tracker page has a "Staff sign-in" button).
Email notifications are built but **off** (`EMAIL_ENABLED=false`); set SMTP values in `.env` to turn them on.

## Setup (server)

```bash
cd /srv/canyon-oaks
scripts/setup-env.sh                  # writes .env with a random DB password + staff token (mode 600)
# copy seed data from the build machine (never committed):
#   rsync -a seed/ minipc:/srv/canyon-oaks/seed/     (canyon_oaks.dump + attachments/)
python3 scripts/fetch_ortho.py data/tiles/ortho.mbtiles 8   # mirror the 2024 ortho tile cache (levels 13-22)
scripts/seed.sh                       # restores the dump, starts everything
docker compose ps                     # all services healthy
curl http://127.0.0.1:8080/api/health
```

`scripts/make_seed.sh` (run on the machine holding the AGO export) builds `seed/` from the local
`canyon_oaks` PostGIS database and the downloaded attachments.

## Update

```bash
cd /srv/canyon-oaks && git pull && docker compose up -d --build
```

## Backups

`scripts/backup.sh` writes `pg_dump -Fc` files to `/srv/canyon-oaks/backups/db/` (14 days kept) and
copies uploaded issue photos to `/srv/canyon-oaks/backups/uploads/`. Install the nightly cron entry
for user `forge` with `scripts/install-cron.sh`:

```
30 2 * * * /srv/canyon-oaks/scripts/backup.sh >> /srv/canyon-oaks/backups/backup.log 2>&1
```

Restore: `docker compose exec -T db pg_restore -U canyon -d canyon_oaks --clean --if-exists < backups/db/<file>.dump`.

## Adding the Cloudflare tunnel route (later)

The stack only listens on `127.0.0.1:8080`. When ready, add a public hostname to the existing tunnel that
points at `http://localhost:8080` (Zero Trust dashboard → Networks → Tunnels → the tunnel → Public Hostname,
or an `ingress` entry `- hostname: <name> / service: http://localhost:8080` above the catch-all rule in the
cloudflared config, then restart cloudflared). Consider protecting `/issues` staff use with Cloudflare Access.
Nothing in this repo changes cloudflared, DNS or Cloudflare settings.

## Data notes

- Source: public ArcGIS Online items owned by `ktfihl824` (read-only export). Table names match the export (`signs`, `fire_hydrants`, `parcels`, …).
- The 5 Survey123 public-view reports are imported into `app.issues` on API start, keeping their `CO-YYMMDD-NNNN` numbers.
- Seed data, the database, ortho tiles, uploads and `.env` are gitignored.
