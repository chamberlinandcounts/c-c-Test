#!/usr/bin/env python3
"""Download the cached Canyon_Oaks_Ortho_2024 tiles (levels 13-22, within the
service fullExtent) from ArcGIS Online into an MBTiles file Martin can serve.
Read-only, polite (default 8 workers), skips 404s, resumable."""
import json, math, sqlite3, sys, threading, time, urllib.request, urllib.error
from concurrent.futures import ThreadPoolExecutor

SVC = "https://tiles.arcgis.com/tiles/POwOJkbM7sfH1VC1/arcgis/rest/services/Canyon_Oaks_Ortho_2024/MapServer"
OUT = sys.argv[1] if len(sys.argv) > 1 else "data/tiles/ortho.mbtiles"
WORKERS = int(sys.argv[2]) if len(sys.argv) > 2 else 8
ZMIN, ZMAX = 13, 22
UA = {"User-Agent": "chamberlin-counts-ortho-mirror/1.0"}

def get(url):
    with urllib.request.urlopen(urllib.request.Request(url, headers=UA), timeout=60) as r:
        return r.read()

meta = json.loads(get(SVC + "?f=json"))
e = meta["fullExtent"]
ti = meta["tileInfo"]
ox, oy, ts = ti["origin"]["x"], ti["origin"]["y"], ti["rows"]

def merc_to_ll(x, y):
    return (x / 6378137.0 * 180 / math.pi,
            (2 * math.atan(math.exp(y / 6378137.0)) - math.pi / 2) * 180 / math.pi)

db = sqlite3.connect(OUT, check_same_thread=False)
db.executescript("""
CREATE TABLE IF NOT EXISTS metadata (name text PRIMARY KEY, value text);
CREATE TABLE IF NOT EXISTS tiles (zoom_level integer, tile_column integer, tile_row integer, tile_data blob);
CREATE UNIQUE INDEX IF NOT EXISTS tile_index ON tiles (zoom_level, tile_column, tile_row);
CREATE TABLE IF NOT EXISTS done (z integer, x integer, y integer, status integer, PRIMARY KEY (z, x, y));
""")
w, s = merc_to_ll(e["xmin"], e["ymin"]); ea, n = merc_to_ll(e["xmax"], e["ymax"])
for k, v in {"name": "Canyon Oaks Ortho 2024", "format": "png", "type": "overlay",
             "minzoom": str(ZMIN), "maxzoom": str(ZMAX), "bounds": f"{w},{s},{ea},{n}",
             "center": f"{(w+ea)/2},{(s+n)/2},17", "attribution": "Chamberlin & Counts UAS imagery 2024",
             "description": "Mixed JPEG/PNG tiles mirrored from the 2024 tile cache"}.items():
    db.execute("INSERT OR REPLACE INTO metadata VALUES (?,?)", (k, v))
db.commit()
done = {(z, x, y) for z, x, y in db.execute("SELECT z,x,y FROM done")}
lock = threading.Lock()
jobs = []
for lod in ti["lods"]:
    z = lod["level"]
    if not ZMIN <= z <= ZMAX: continue
    span = lod["resolution"] * ts
    c0 = math.floor((e["xmin"] - ox) / span); c1 = math.floor((e["xmax"] - ox) / span)
    r0 = math.floor((oy - e["ymax"]) / span); r1 = math.floor((oy - e["ymin"]) / span)
    jobs += [(z, c, r) for r in range(r0, r1 + 1) for c in range(c0, c1 + 1) if (z, c, r) not in done]
print(f"{len(done)} already done, {len(jobs)} to fetch", flush=True)
stats = {"ok": 0, "404": 0, "err": 0, "bytes": 0}
pending = []

def work(job):
    z, x, y = job
    for attempt in range(4):
        try:
            data = get(f"{SVC}/tile/{z}/{y}/{x}")
            with lock:
                pending.append((z, x, (1 << z) - 1 - y, data, 200)); stats["ok"] += 1; stats["bytes"] += len(data)
            return
        except urllib.error.HTTPError as ex:
            if ex.code == 404:
                with lock: pending.append((z, x, y, None, 404)); stats["404"] += 1
                return
            time.sleep(2 ** attempt)
        except Exception:
            time.sleep(2 ** attempt)
    with lock: stats["err"] += 1

def flush():
    with lock:
        batch = pending[:]; pending.clear()
    for z, x, ty, data, st in batch:
        if st == 200:
            db.execute("INSERT OR REPLACE INTO tiles VALUES (?,?,?,?)", (z, x, ty, data))
            db.execute("INSERT OR REPLACE INTO done VALUES (?,?,?,200)", (z, x, (1 << z) - 1 - ty))
        else:
            db.execute("INSERT OR REPLACE INTO done VALUES (?,?,?,404)", (z, x, ty))
    db.commit()

t0 = time.time()
with ThreadPoolExecutor(WORKERS) as ex:
    futs = [ex.submit(work, j) for j in jobs]
    for i, f in enumerate(futs, 1):
        f.result()
        if i % 2000 == 0:
            flush(); print(f"{i}/{len(jobs)} {stats} {time.time()-t0:.0f}s", flush=True)
flush()
print("DONE", stats, f"{time.time()-t0:.0f}s", flush=True)
