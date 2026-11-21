// Shared helpers: header/nav, API calls, formatting.
(function () {
  const PAGES = [
    ["/", "Community Map", "Map"],
    ["/assets", "Asset Dashboard", "Assets"],
    ["/issues", "Issue Tracker", "Issues"],
    ["/report", "Report an Issue", "Report"],
  ];
  function header() {
    const el = document.querySelector("header.topbar");
    if (!el) return;
    const here = location.pathname.replace(/\.html$/, "").replace(/\/index$/, "/") || "/";
    el.innerHTML = `
      <a class="brand" href="/"><span class="mark">C&amp;C</span>
        <span><span class="name">Canyon Oaks</span><br><span class="sub">Chamberlin &amp; Counts</span></span></a>
      <nav class="nav">${PAGES.map(([h, t, sh]) => `<a href="${h}" class="${here === h ? "active" : ""}"><span class="long">${t}</span><span class="short">${sh}</span></a>`).join("")}</nav>
      <span class="spacer"></span>`;
  }
  async function api(path, opts = {}) {
    const headers = Object.assign({}, opts.headers || {});
    const tok = sessionStorage.getItem("cc_admin_token");
    if (tok && opts.admin !== false) headers["Authorization"] = "Bearer " + tok;
    const res = await fetch(path, Object.assign({}, opts, { headers }));
    const ct = res.headers.get("content-type") || "";
    const body = ct.includes("json") ? await res.json() : await res.text();
    if (!res.ok) {
      const err = new Error((body && body.error) || res.statusText);
      err.status = res.status;
      throw err;
    }
    return body;
  }
  const esc = (s) => String(s ?? "").replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c]));
  const TZ = "America/Los_Angeles";
  function fmtDate(v) {
    if (v == null || v === "") return "";
    const d = typeof v === "number" ? new Date(v) : new Date(String(v).replace(" ", "T"));
    if (isNaN(d)) return String(v);
    return d.toLocaleString("en-US", { timeZone: TZ, month: "numeric", day: "numeric", year: "numeric", hour: "numeric", minute: "2-digit" });
  }
  function fmtNum(n, max = 1) { return Number(n).toLocaleString("en-US", { maximumFractionDigits: max }); }
  function lightbox(src) {
    let lb = document.querySelector(".lightbox");
    if (!lb) {
      lb = document.createElement("div");
      lb.className = "lightbox";
      lb.innerHTML = "<img alt=''>";
      lb.onclick = () => (lb.style.display = "none");
      document.body.appendChild(lb);
    }
    lb.querySelector("img").src = src;
    lb.style.display = "grid";
  }
  window.CC = { header, api, esc, fmtDate, fmtNum, lightbox, TZ };
  document.addEventListener("DOMContentLoaded", header);
})();
