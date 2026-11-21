// Map builder: basemap + ortho + every Canyon Oaks layer, styled from the
// renderers saved out of the ArcGIS Online web map (config/layers.json).
(function () {
  const ORIGIN = location.origin;
  const BASEMAP = "https://tiles.openfreemap.org/styles/positron";
  const BOUNDS = [[-121.7865, 39.7550], [-121.7540, 39.7715]];
  let CONFIG = null;

  async function loadConfig() {
    if (!CONFIG) CONFIG = await (await fetch("/config/layers.json")).json();
    return CONFIG;
  }

  // ---------- symbol rendering (canvas) ----------
  function drawMarker(sym, ratio = 2) {
    const size = Math.max(4, sym.size || 8);
    const ow = sym.outline ? Math.max(0, sym.outlineWidth || 0) : 0;
    const px = Math.ceil((size + ow * 2 + 2) * ratio);
    const c = document.createElement("canvas");
    c.width = c.height = px;
    const g = c.getContext("2d");
    g.scale(ratio, ratio);
    const m = px / ratio / 2, r = size / 2;
    g.beginPath();
    switch (sym.shape) {
      case "square": g.rect(m - r, m - r, size, size); break;
      case "diamond": g.moveTo(m, m - r); g.lineTo(m + r, m); g.lineTo(m, m + r); g.lineTo(m - r, m); g.closePath(); break;
      case "triangle": g.moveTo(m, m - r); g.lineTo(m + r, m + r * 0.8); g.lineTo(m - r, m + r * 0.8); g.closePath(); break;
      case "cross": g.rect(m - r / 4, m - r, r / 2, size); g.rect(m - r, m - r / 4, size, r / 2); break;
      case "x": g.translate(m, m); g.rotate(Math.PI / 4); g.rect(-r / 4, -r, r / 2, size); g.rect(-r, -r / 4, size, r / 2); g.setTransform(ratio, 0, 0, ratio, 0, 0); break;
      default: g.arc(m, m, r, 0, Math.PI * 2);
    }
    g.fillStyle = sym.color || "#888";
    g.fill();
    if (ow > 0) { g.lineWidth = ow; g.strokeStyle = sym.outline; g.stroke(); }
    return c;
  }
  function swatch(sym) {
    if (!sym) return "";
    if (sym.kind === "icon") return `<img src="/icons/${sym.icon}" alt="">`;
    if (sym.kind === "marker") return `<img src="${drawMarker(sym, 2).toDataURL()}" alt="">`;
    if (sym.kind === "line") return `<svg class="sw" viewBox="0 0 20 20"><line x1="1" y1="15" x2="19" y2="5" stroke="${sym.color}" stroke-width="${Math.max(1.5, sym.width)}" ${sym.dash ? 'stroke-dasharray="4 3"' : ""}/></svg>`;
    if (sym.kind === "fill") return `<svg class="sw" viewBox="0 0 20 20"><rect x="2" y="3" width="16" height="14" rx="2" fill="${sym.color}" stroke="${sym.outline || "none"}" stroke-width="${Math.max(1, sym.outlineWidth || 1)}"/></svg>`;
    return "";
  }

  async function ensureImage(map, sym) {
    if (sym.kind === "icon") {
      const id = "i:" + sym.icon;
      if (!map.hasImage(id)) {
        const img = await map.loadImage("/icons/" + sym.icon);
        if (!map.hasImage(id)) map.addImage(id, img.data);
      }
      const nat = map.getImage(id)?.data?.width || 64;
      return { id, scale: (sym.width || 20) / nat };
    }
    const id = "m:" + JSON.stringify(sym);
    if (!map.hasImage(id)) {
      const c = drawMarker(sym, 2);
      map.addImage(id, c.getContext("2d").getImageData(0, 0, c.width, c.height), { pixelRatio: 2 });
    }
    return { id, scale: 1 };
  }

  function classesOf(r) {
    if (r.type === "simple") return [{ label: r.label || "", symbol: r.symbol }];
    const out = (r.classes || []).map((c) => ({ label: c.label, value: c.value, symbol: c.symbol }));
    if (r.default) out.push({ label: r.defaultLabel || "Other", symbol: r.default, isDefault: true });
    return out;
  }

  // ---------- layers ----------
  async function addDataLayer(map, L) {
    const src = "src:" + L.id;
    if (!map.getSource(src)) {
      map.addSource(src, { type: "vector", tiles: [`${ORIGIN}/tiles/${L.table}/{z}/{x}/{y}`], minzoom: 0, maxzoom: 20 });
    }
    const r = L.renderer;
    const vis = L.visible === false ? "none" : "visible";
    const ids = [];
    if (L.geometry === "point") {
      const cls = classesOf(r);
      const imgs = [];
      for (const c of cls) imgs.push(await ensureImage(map, c.symbol));
      let image, size, filter;
      if (r.type === "uniqueValue") {
        const f = ["to-string", ["get", r.field]];
        const vals = cls.filter((c) => !c.isDefault);
        const def = cls.find((c) => c.isDefault);
        const defIdx = cls.indexOf(def);
        image = ["match", f, ...vals.flatMap((c, i) => [String(c.value), imgs[i].id]), def ? imgs[defIdx].id : ""];
        size = ["match", f, ...vals.flatMap((c, i) => [String(c.value), imgs[i].scale]), def ? imgs[defIdx].scale : 1];
        if (!def) filter = ["match", f, vals.map((c) => String(c.value)), true, false];
      } else {
        image = imgs[0].id; size = imgs[0].scale;
      }
      const id = `lyr:${L.id}:sym`;
      map.addLayer(Object.assign({
        id, type: "symbol", source: src, "source-layer": L.table,
        layout: { "icon-image": image, "icon-size": size, "icon-allow-overlap": true, "icon-ignore-placement": true, visibility: vis },
        paint: { "icon-opacity": L.opacity ?? 1 },
      }, filter ? { filter } : {}));
      ids.push(id);
    } else if (L.geometry === "line") {
      const s = r.symbol || {};
      const id = `lyr:${L.id}:line`;
      map.addLayer({ id, type: "line", source: src, "source-layer": L.table,
        layout: { visibility: vis, "line-cap": "round", "line-join": "round" },
        paint: Object.assign({ "line-color": s.color || "#555", "line-width": Math.max(1.2, s.width || 1), "line-opacity": L.opacity ?? 1 }, s.dash ? { "line-dasharray": [3, 2] } : {}) });
      ids.push(id);
    } else {
      const s = r.symbol || {};
      const fid = `lyr:${L.id}:fill`, lid = `lyr:${L.id}:line`;
      map.addLayer({ id: fid, type: "fill", source: src, "source-layer": L.table, layout: { visibility: vis },
        paint: { "fill-color": s.color || "rgba(0,0,0,0)", "fill-opacity": L.opacity ?? 1 } });
      map.addLayer({ id: lid, type: "line", source: src, "source-layer": L.table, layout: { visibility: vis },
        paint: { "line-color": s.outline || "#333", "line-width": Math.max(1, s.outlineWidth || 1), "line-opacity": L.opacity ?? 1 } });
      ids.push(fid, lid);
    }
    L._mapLayers = ids;
  }

  function addOrtho(map) {
    map.addSource("ortho", { type: "raster", tiles: [`${ORIGIN}/tiles/ortho/{z}/{x}/{y}`], tileSize: 256, minzoom: 13, maxzoom: 22,
      bounds: [-121.787, 39.7540, -121.7535, 39.7722], attribution: "Imagery © 2024 Chamberlin &amp; Counts" });
    // Insert below the basemap's first symbol (label) layer so street names stay readable.
    const firstSymbol = map.getStyle().layers.find((l) => l.type === "symbol");
    map.addLayer({ id: "ortho", type: "raster", source: "ortho", paint: { "raster-opacity": 1, "raster-fade-duration": 150 } }, firstSymbol && firstSymbol.id);
  }

  // ---------- pop-ups ----------
  const GNSS = /^esri(gnss|snsr)_/;
  function fmtVal(L, f, v) {
    if (v == null || v === "") return "";
    const dom = L.domains && L.domains[f.name];
    if (dom && dom[String(v)] != null) return CC.esc(dom[String(v)]);
    if (f.date || /date|time/i.test(f.name) && typeof v === "string" && /^\d{4}-\d\d-\d\d/.test(v)) return CC.esc(CC.fmtDate(v));
    if (typeof v === "number") return CC.esc(Number.isInteger(v) ? String(v) : CC.fmtNum(v, 3));
    return CC.esc(v);
  }
  function title(L, p) {
    const t = (L.popup && L.popup.title) || L.title;
    return t.replace(/\{([^}]+)\}/g, (_, k) => {
      const dom = L.domains && L.domains[k];
      const v = p[k];
      return v == null ? "" : dom && dom[String(v)] != null ? dom[String(v)] : v;
    });
  }
  async function popupHtml(L, fid) {
    const fc = await CC.api(`/api/features/${L.table}?fid=${encodeURIComponent(fid)}`);
    const feat = fc.features[0];
    if (!feat) return `<div class="pop"><div class="ph">${CC.esc(L.title)}</div><div class="pb">Feature not found.</div></div>`;
    const p = feat.properties;
    const fields = (L.popup && L.popup.fields) || Object.keys(p).map((k) => ({ name: k, label: k }));
    const main = fields.filter((f) => !GNSS.test(f.name)), gnss = fields.filter((f) => GNSS.test(f.name));
    const rows = (fs) => fs.map((f) => `<tr><td>${CC.esc(f.label)}</td><td>${fmtVal(L, f, p[f.name])}</td></tr>`).join("");
    let photos = "";
    if (L.photos && p.objectid != null) {
      try {
        const att = await CC.api(`/api/attachments/${L.table}/${p.objectid}`);
        if (att.length) photos = `<div class="photos">${att.map((a) => `<img loading="lazy" src="${a.url}" alt="${CC.esc(a.name || "photo")}" onclick="CC.lightbox(this.src)">`).join("")}</div>`;
      } catch (e) { /* no photos */ }
    }
    return `<div class="pop"><div class="ph">${CC.esc(title(L, p))}<small>${CC.esc(L.title)}</small></div>
      <div class="pb">${photos}<table>${rows(main)}</table>
      ${gnss.length ? `<details><summary>GNSS / collection metadata (${gnss.length})</summary><table>${rows(gnss)}</table></details>` : ""}</div></div>`;
  }

  function wirePopups(map, layers) {
    const byMapLayer = {};
    layers.forEach((L) => (L._mapLayers || []).forEach((id) => (byMapLayer[id] = L)));
    const ids = Object.keys(byMapLayer);
    const popup = new maplibregl.Popup({ maxWidth: "360px", closeButton: true });
    let hits = [], idx = 0, lngLat;
    async function show() {
      const h = hits[idx];
      popup.setLngLat(lngLat).setHTML(`<div class="pop"><div class="ph">${CC.esc(h.L.title)}</div><div class="pb">Loading…</div></div>`).addTo(map);
      let html = await popupHtml(h.L, h.fid);
      if (hits.length > 1) html = html.replace(/<\/div>$/, `<div class="pager"><button class="btn small" data-p="-1">‹ Prev</button><span class="muted">${idx + 1} of ${hits.length}</span><button class="btn small" data-p="1">Next ›</button></div></div>`);
      popup.setHTML(html);
      popup.getElement().querySelectorAll("[data-p]").forEach((b) => (b.onclick = () => { idx = (idx + hits.length + Number(b.dataset.p)) % hits.length; show(); }));
    }
    map.on("click", (e) => {
      const visible = ids.filter((id) => map.getLayer(id) && map.getLayoutProperty(id, "visibility") !== "none");
      const feats = map.queryRenderedFeatures([[e.point.x - 6, e.point.y - 6], [e.point.x + 6, e.point.y + 6]], { layers: visible });
      const seen = new Set();
      hits = [];
      for (const f of feats) {
        const L = byMapLayer[f.layer.id];
        const fid = f.id ?? f.properties.ogc_fid;
        const key = L.id + ":" + fid;
        if (fid == null || seen.has(key)) continue;
        seen.add(key);
        hits.push({ L, fid });
      }
      if (!hits.length) return;
      idx = 0; lngLat = e.lngLat;
      show();
    });
    map.on("mousemove", (e) => {
      const visible = ids.filter((id) => map.getLayer(id) && map.getLayoutProperty(id, "visibility") !== "none" && !id.endsWith(":fill"));
      const f = map.queryRenderedFeatures(e.point, { layers: visible });
      map.getCanvas().style.cursor = f.length ? "pointer" : "";
    });
    return { openFeature: async (L, fid, ll) => { hits = [{ L, fid }]; idx = 0; lngLat = ll; await show(); } };
  }

  // ---------- legend / layer list ----------
  function buildLegend(container, map, cfg) {
    let html = `<div class="layer-group"><div class="lyr"><label><input type="checkbox" data-ortho checked> Canyon Oaks Ortho 2024</label></div>
      <div class="opacity">Opacity <input type="range" min="0" max="100" value="100" data-ortho-op></div></div>`;
    const groups = [];
    [...cfg.layers].reverse().forEach((L) => {
      if (L.id === "ortho") return;
      const g = L.group || "";
      let grp = groups.find((x) => x.name === g && x.last);
      if (!grp) { groups.forEach((x) => (x.last = false)); grp = { name: g, items: [], last: true }; groups.push(grp); }
      grp.items.push(L);
    });
    for (const g of groups) {
      html += `<div class="layer-group">${g.name ? `<div class="gtitle">${CC.esc(g.name)}</div>` : ""}`;
      for (const L of g.items) {
        const cls = classesOf(L.renderer);
        const legend = cls.length > 1 || (cls[0] && cls[0].label)
          ? `<div class="legend">${cls.map((c) => `<div class="legend-row">${swatch(c.symbol)}<span>${CC.esc(c.label)}</span></div>`).join("")}</div>` : "";
        const single = cls.length === 1 && !cls[0].label ? swatch(cls[0].symbol) : "";
        html += `<div class="lyr"><label><input type="checkbox" data-layer="${L.id}" ${L.visible === false ? "" : "checked"}>${single ? `<span class="legend-row">${single}</span>` : ""}${CC.esc(L.title)}</label>${legend}</div>`;
      }
      html += `</div>`;
    }
    container.innerHTML = html;
    container.querySelectorAll("[data-layer]").forEach((cb) => (cb.onchange = () => {
      const L = cfg.layers.find((x) => x.id === cb.dataset.layer);
      (L._mapLayers || []).forEach((id) => map.setLayoutProperty(id, "visibility", cb.checked ? "visible" : "none"));
    }));
    container.querySelector("[data-ortho]").onchange = (e) => map.setLayoutProperty("ortho", "visibility", e.target.checked ? "visible" : "none");
    container.querySelector("[data-ortho-op]").oninput = (e) => map.setPaintProperty("ortho", "raster-opacity", e.target.value / 100);
  }

  async function create(el, opts = {}) {
    const cfg = await loadConfig();
    const map = new maplibregl.Map({
      container: el, style: BASEMAP, bounds: BOUNDS, fitBoundsOptions: { padding: 20 }, maxZoom: 22,
      attributionControl: { compact: true }, cooperativeGestures: !!opts.cooperative,
    });
    map.addControl(new maplibregl.NavigationControl({ showCompass: false }), "top-right");
    map.addControl(new maplibregl.ScaleControl({ unit: "imperial" }), "bottom-left");
    await new Promise((res) => map.once("load", res));
    const layers = cfg.layers.filter((l) => l.id !== "ortho" && (!opts.only || opts.only.includes(l.id)));
    if (opts.ortho !== false) addOrtho(map);
    if (opts.layers !== false) for (const L of layers) await addDataLayer(map, L);
    const pop = opts.layers !== false ? wirePopups(map, layers) : null;
    return { map, cfg, layers, popups: pop, buildLegend: (c) => buildLegend(c, map, cfg) };
  }

  window.CCMap = { create, loadConfig, drawMarker, swatch, BOUNDS };
})();
