#!/usr/bin/env python3
"""Translate the saved ArcGIS Online definitions (web map renderers, pop-ups,
field domains, dashboard widgets) into web/site/config/*.json + icon PNGs used
by the MapLibre front end. Input: the read-only AGO export directory."""
import base64, hashlib, json, os, re, sys

EXPORT = sys.argv[1] if len(sys.argv) > 1 else "/workspace/canyon-oaks"
OUT = os.path.join(os.path.dirname(__file__), "..", "web", "site")
WEBMAP = "22e14482c812412daae128402aa1f234"

TABLES = {  # meta layer name -> PostGIS table (see load_postgis.sh in the export)
    "drainage_practice__drain_inlet": "drain_inlets",
    "drainage_practice__stormdrain_manhole": "storm_manholes",
    "drainage_practice__storm_drain_lines": "storm_drain_lines",
    "canyon_oaks_report_an_issue_public_view__survey": "issue_reports",
    "signs_practice_with_photos__signs": "signs",
    "monuments_practice__monuments_practice": "monuments",
    "control_practice__control_practice": "control_points",
    "irrigation_practice__irrigation": "irrigation",
    "hma_overlay_practice__repairs_hma": "hma_overlay_repairs",
    "sewer_practice__manhole": "sewer_manholes",
    "sewer_practice__lines": "sewer_lines",
    "parcels_practice__parcels_practice": "parcels",
    "aerial_targets_practice__aerial_targets_practice": "aerial_targets",
    "fire_hydrants_practice__fire_hydrants": "fire_hydrants",
    "full_repair_practice__full_repair": "full_repair",
    "hydrant_coverage_300ft_practice__bufferedfeatures": "hydrant_coverage_300ft",
}
ATTACH = {"signs", "fire_hydrants", "irrigation", "issue_reports"}

summary = json.load(open(f"{EXPORT}/meta/_summary.json"))
url2meta = {row[5]: row[0] for row in summary if len(row) >= 6}
webmap = json.load(open(next(f"{EXPORT}/items/{f}" for f in os.listdir(f"{EXPORT}/items") if f.endswith(f"{WEBMAP}__data.json"))))

def pt2px(v): return round((v or 0) * 4 / 3, 2)

def color(c):
    if not c: return None
    r, g, b, a = (list(c) + [255])[:4]
    return f"rgba({r},{g},{b},{round(a/255, 3)})"

icons = {}
def symbol(s):
    if not s: return None
    t = s.get("type")
    if t == "esriSMS":
        ol = s.get("outline") or {}
        return {"kind": "marker", "shape": (s.get("style") or "esriSMSCircle").replace("esriSMS", "").lower(),
                "color": color(s.get("color")), "size": pt2px(s.get("size", 8)),
                "outline": color(ol.get("color")), "outlineWidth": pt2px(ol.get("width", 0))}
    if t == "esriPMS":
        data = base64.b64decode(s["imageData"])
        name = hashlib.sha1(data).hexdigest()[:12] + ".png"
        open(os.path.join(OUT, "icons", name), "wb").write(data)
        icons[name] = True
        return {"kind": "icon", "icon": name, "width": pt2px(s.get("width", 15)), "height": pt2px(s.get("height", 15))}
    if t == "esriSLS":
        return {"kind": "line", "color": color(s.get("color")), "width": pt2px(s.get("width", 1)),
                "dash": None if s.get("style") in (None, "esriSLSSolid") else s.get("style").replace("esriSLS", "").lower()}
    if t == "esriSFS":
        ol = s.get("outline") or {}
        return {"kind": "fill", "color": color(s.get("color")) if s.get("style") != "esriSFSNull" else "rgba(0,0,0,0)",
                "outline": color(ol.get("color")), "outlineWidth": pt2px(ol.get("width", 1))}
    return {"kind": "unsupported", "raw": t}

def renderer(dr):
    r = (dr or {}).get("renderer") or {}
    if r.get("type") == "simple":
        return {"type": "simple", "symbol": symbol(r.get("symbol")), "label": r.get("label") or ""}
    if r.get("type") == "uniqueValue":
        classes = [{"value": u.get("value"), "label": u.get("label") or u.get("value"), "symbol": symbol(u.get("symbol"))}
                   for u in r.get("uniqueValueInfos", [])]
        return {"type": "uniqueValue", "field": (r.get("field1") or "").lower(), "classes": classes,
                "default": symbol(r.get("defaultSymbol")), "defaultLabel": r.get("defaultLabel") or "Other"}
    return {"type": "unsupported", "raw": r.get("type")}

def domains(meta):
    out = {}
    for f in meta.get("fields", []):
        d = f.get("domain") or {}
        if d.get("type") == "codedValue":
            out[f["name"].lower()] = {str(c["code"]): c["name"] for c in d["codedValues"]}
    return out

def aliases(meta):
    return {f["name"].lower(): f.get("alias") or f["name"] for f in meta.get("fields", [])}

def popup(pi, meta):
    if not pi: return None
    al = aliases(meta)
    fields = []
    for fi in pi.get("fieldInfos", []):
        if not fi.get("visible"): continue
        n = fi["fieldName"]
        if n.startswith("expression/"): continue
        fields.append({"name": n.lower(), "label": fi.get("label") or al.get(n.lower(), n),
                       "date": "dateFormat" in (fi.get("format") or {})})
    title = re.sub(r"\{([^}]+)\}", lambda m: "{" + m.group(1).lower() + "}", pi.get("title") or "")
    return {"title": title, "fields": fields}

layers = []
def walk(ops, group=None):
    for op in ops:
        if op.get("layers"):
            walk(op["layers"], op.get("title")); continue
        url = op.get("url", "")
        if "MapServer" in url:
            layers.append({"id": "ortho", "title": op["title"], "type": "raster", "group": None}); continue
        mname = url2meta.get(url)
        if not mname or mname not in TABLES:
            print("skip", op.get("title"), url); continue
        meta = json.load(open(f"{EXPORT}/meta/{mname}.json"))
        dr = (op.get("layerDefinition") or {}).get("drawingInfo") or meta.get("drawingInfo")
        table = TABLES[mname]
        gt = {"esriGeometryPoint": "point", "esriGeometryPolyline": "line", "esriGeometryPolygon": "polygon"}[meta["geometryType"]]
        layers.append({"id": table, "table": table, "title": op["title"], "group": group, "geometry": gt,
                       "visible": op.get("visibility", True), "opacity": op.get("opacity", 1),
                       "renderer": renderer(dr), "popup": popup(op.get("popupInfo"), meta),
                       "domains": domains(meta), "photos": table in ATTACH})
walk(webmap["operationalLayers"])

# Issue Reports web map: keep its renderer (status colours) + pop-up template for the tracker.
iwm = json.load(open(next(f"{EXPORT}/items/{f}" for f in os.listdir(f"{EXPORT}/items") if "Issue_Reports" in f and f.endswith("__data.json"))))
iop = iwm["operationalLayers"][0]
issue_style = renderer(iop["layerDefinition"]["drawingInfo"])

os.makedirs(os.path.join(OUT, "config"), exist_ok=True)
json.dump({"generated_from": "ArcGIS Online web map " + WEBMAP, "layers": layers,
           "initialExtent": webmap.get("initialState", {}).get("viewpoint", {}).get("targetGeometry")},
          open(os.path.join(OUT, "config", "layers.json"), "w"), indent=1)
json.dump(issue_style, open(os.path.join(OUT, "config", "issue_style.json"), "w"), indent=1)
for l in layers:
    r = l.get("renderer", {})
    print(f"{l['id']:24} {l.get('geometry',''):8} {r.get('type','')}: "
          + json.dumps(r.get('symbol') or [ (c['label'], c['symbol']['kind']) for c in r.get('classes', [])][:6])[:160])
print(len(icons), "icons")
