// Minimal dependency-free SVG charts for the dashboards.
(function () {
  const PALETTE = ["#1d5fa8", "#e07a1f", "#2a9d8f", "#c0392b", "#8e5ac8", "#d4a017", "#4c8c2b", "#d1477a", "#5b6c7d", "#17becf", "#a0522d", "#7f8c8d"];
  function bar(el, data, { color = "#1d5fa8", onClick } = {}) {
    const rowH = 24, labelW = Math.min(190, Math.max(70, ...data.map((d) => String(d.label).length * 7))), w = 520;
    const max = Math.max(1, ...data.map((d) => d.value));
    const h = data.length * rowH + 8;
    const plotW = w - labelW - 46;
    el.innerHTML = `<svg viewBox="0 0 ${w} ${h}" width="100%" style="max-width:${w}px;max-height:${h * 1.15}px" role="img">
      ${data.map((d, i) => {
        const y = i * rowH + 4, bw = Math.max(2, (d.value / max) * plotW);
        return `<g data-i="${i}" style="cursor:${onClick ? "pointer" : "default"}">
          <text x="${labelW - 8}" y="${y + 15}" text-anchor="end" font-size="12" fill="#4a5663">${CC.esc(d.label)}</text>
          <rect x="${labelW}" y="${y + 3}" width="${bw}" height="${rowH - 8}" rx="3" fill="${d.color || color}"><title>${CC.esc(d.label)}: ${d.value}</title></rect>
          <text x="${labelW + bw + 6}" y="${y + 15}" font-size="12" fill="#17212b" font-weight="600">${d.value}</text></g>`;
      }).join("")}</svg>`;
    if (onClick) el.querySelectorAll("[data-i]").forEach((g) => (g.onclick = () => onClick(data[g.dataset.i])));
  }
  function pie(el, data) {
    const total = data.reduce((s, d) => s + d.value, 0) || 1;
    let a = -Math.PI / 2;
    const r = 80, cx = 90, cy = 90;
    const slices = data.map((d, i) => {
      const frac = d.value / total, a2 = a + frac * Math.PI * 2;
      const large = frac > 0.5 ? 1 : 0;
      const p = frac >= 0.9999
        ? `<circle cx="${cx}" cy="${cy}" r="${r}" fill="${PALETTE[i % PALETTE.length]}"/>`
        : `<path d="M${cx},${cy} L${cx + r * Math.cos(a)},${cy + r * Math.sin(a)} A${r},${r} 0 ${large} 1 ${cx + r * Math.cos(a2)},${cy + r * Math.sin(a2)} Z" fill="${PALETTE[i % PALETTE.length]}" stroke="#fff" stroke-width="1.5"><title>${CC.esc(d.label)}: ${d.value}</title></path>`;
      a = a2;
      return p;
    }).join("");
    el.innerHTML = `<div style="display:flex;gap:14px;align-items:center;flex-wrap:wrap">
      <svg viewBox="0 0 180 180" width="170" height="170" role="img">${slices}</svg>
      <div class="chart-legend">${data.map((d, i) => `<div><span class="sw" style="background:${PALETTE[i % PALETTE.length]}"></span>${CC.esc(d.label)} <b>${d.value}</b> <span class="muted">(${Math.round((d.value / total) * 100)}%)</span></div>`).join("")}</div></div>`;
  }
  window.CCCharts = { bar, pie, PALETTE };
})();
