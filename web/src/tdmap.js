// Taitel–Dukler map: the classifier's own transition boundaries, drawn by
// classifying a grid of (j_gas, j_liq) points, with every live cell plotted
// on top so you watch the flow migrate between regimes as the transient runs.

import { REGIME_COLORS, REGIME_SHORT as SHORT, T, rgba, setFont } from "./theme.js";

export class TDMap {
  constructor(canvas, classifyPoint) {
    this.cv = canvas;
    this.ctx = canvas.getContext("2d");
    this.classify = classifyPoint;
    this.jg = [-2, 1.7]; // log10 m/s
    this.jl = [-2.3, 1];
    this.bgKey = "";
  }

  buildBackground(d, p) {
    const key = `${d.toFixed(4)}|${(p / 1e4).toFixed(0)}`;
    if (key === this.bgKey) return;
    this.bgKey = key;
    const W = 96;
    const H = 76;
    const off = new OffscreenCanvas(W, H);
    const octx = off.getContext("2d");
    const img = octx.createImageData(W, H);
    const codes = new Uint8Array(W * H);
    for (let iy = 0; iy < H; iy++) {
      for (let ix = 0; ix < W; ix++) {
        const jg = 10 ** (this.jg[0] + (ix / (W - 1)) * (this.jg[1] - this.jg[0]));
        const jl = 10 ** (this.jl[1] - (iy / (H - 1)) * (this.jl[1] - this.jl[0]));
        const r = this.classify(jg, jl, d, 0, 1, p);
        codes[iy * W + ix] = r;
        const c = REGIME_COLORS[r] || "#999";
        const k = 4 * (iy * W + ix);
        img.data[k] = parseInt(c.slice(1, 3), 16);
        img.data[k + 1] = parseInt(c.slice(3, 5), 16);
        img.data[k + 2] = parseInt(c.slice(5, 7), 16);
        img.data[k + 3] = 92;
      }
    }
    octx.putImageData(img, 0, 0);
    this.bg = off;
    this.codes = codes;
    this.gw = W;
    this.gh = H;
    // label anchors: centroid of each region that is big enough to name
    const acc = new Map();
    for (let i = 0; i < codes.length; i++) {
      const r = codes[i];
      const a = acc.get(r) || { n: 0, x: 0, y: 0 };
      a.n++;
      a.x += i % W;
      a.y += (i / W) | 0;
      acc.set(r, a);
    }
    this.labels = [...acc.entries()]
      .filter(([, a]) => a.n > 0.05 * codes.length)
      .map(([r, a]) => ({ r, fx: a.x / a.n / W, fy: a.y / a.n / H }));
  }

  draw(view, diameter) {
    const { ctx, cv } = this;
    const dpr = devicePixelRatio;
    const w = cv.width;
    const h = cv.height;
    const L = 42 * dpr;
    const B = 30 * dpr;
    const TOP = 20 * dpr;
    const R = 10 * dpr;
    const pw = w - L - R;
    const ph = h - TOP - B;
    ctx.clearRect(0, 0, w, h);
    ctx.fillStyle = T.cream;
    ctx.fillRect(0, 0, w, h);

    setFont(ctx, 11 * dpr, "600");
    ctx.fillStyle = T.ink;
    ctx.fillText("Taitel–Dukler map", 8 * dpr, 13 * dpr);

    if (diameter) this.buildBackground(diameter, 2e5);
    if (this.bg) {
      ctx.imageSmoothingEnabled = true;
      ctx.drawImage(this.bg, L, TOP, pw, ph);
    }

    // region labels
    if (this.labels) {
      setFont(ctx, 9.5 * dpr);
      ctx.textAlign = "center";
      for (const l of this.labels) {
        const x = L + l.fx * pw;
        const y = TOP + l.fy * ph;
        ctx.fillStyle = "rgba(244,239,228,0.72)";
        const label = SHORT[l.r];
        const tw = ctx.measureText(label).width;
        ctx.fillRect(x - tw / 2 - 3 * dpr, y - 7 * dpr, tw + 6 * dpr, 12 * dpr);
        ctx.fillStyle = T.ink2;
        ctx.fillText(label, x, y + 2.5 * dpr);
      }
      ctx.textAlign = "left";
    }

    // axes
    ctx.strokeStyle = T.line;
    ctx.lineWidth = 1.1 * dpr;
    ctx.strokeRect(L, TOP, pw, ph);
    setFont(ctx, 9.5 * dpr);
    ctx.fillStyle = T.ink3;
    ctx.textAlign = "center";
    for (let e = Math.ceil(this.jg[0]); e <= this.jg[1]; e++) {
      const x = L + ((e - this.jg[0]) / (this.jg[1] - this.jg[0])) * pw;
      ctx.strokeStyle = rgba(T.ink3, 0.25);
      ctx.beginPath();
      ctx.moveTo(x, TOP + ph);
      ctx.lineTo(x, TOP + ph + 3 * dpr);
      ctx.stroke();
      ctx.fillText(`10${sup(e)}`, x, h - B + 15 * dpr);
    }
    ctx.textAlign = "right";
    for (let e = Math.ceil(this.jl[0]); e <= this.jl[1]; e++) {
      const y = TOP + ((this.jl[1] - e) / (this.jl[1] - this.jl[0])) * ph;
      ctx.beginPath();
      ctx.moveTo(L - 3 * dpr, y);
      ctx.lineTo(L, y);
      ctx.stroke();
      ctx.fillText(`10${sup(e)}`, L - 6 * dpr, y + 3 * dpr);
    }
    ctx.textAlign = "left";
    ctx.fillStyle = T.ink2;
    setFont(ctx, 9.5 * dpr);
    ctx.textAlign = "center";
    ctx.fillText("j gas  [m/s]", L + pw / 2, h - 4 * dpr);
    ctx.save();
    ctx.translate(11 * dpr, TOP + ph / 2);
    ctx.rotate(-Math.PI / 2);
    ctx.fillText("j liquid  [m/s]", 0, 0);
    ctx.restore();
    ctx.textAlign = "left";

    if (!view) return;
    const xat = (jg) => L + ((Math.log10(jg) - this.jg[0]) / (this.jg[1] - this.jg[0])) * pw;
    const yat = (jl) => TOP + ((this.jl[1] - Math.log10(jl)) / (this.jl[1] - this.jl[0])) * ph;
    ctx.fillStyle = rgba(T.ink, 0.62);
    for (let i = 0; i < view.n; i += 2) {
      const a = view.alpha[i];
      const jg = Math.abs(a * view.vg[i]);
      const jl = Math.abs((1 - a) * view.vl[i]);
      if (jg < 1e-4 || jl < 1e-4) continue;
      const x = xat(jg);
      const y = yat(jl);
      if (x < L || x > L + pw || y < TOP || y > TOP + ph) continue;
      ctx.beginPath();
      ctx.arc(x, y, 1.7 * dpr, 0, 7);
      ctx.fill();
    }
    // probes stand out, tying the map to the strip charts
    (view.probes || []).forEach((cell, k) => {
      const a = view.alpha[cell];
      const jg = Math.abs(a * view.vg[cell]);
      const jl = Math.abs((1 - a) * view.vl[cell]);
      if (jg < 1e-4 || jl < 1e-4) return;
      const x = xat(jg);
      const y = yat(jl);
      if (x < L || x > L + pw || y < TOP || y > TOP + ph) return;
      ctx.beginPath();
      ctx.arc(x, y, 4.2 * dpr, 0, 7);
      ctx.fillStyle = T.cream;
      ctx.fill();
      ctx.strokeStyle = T.probe[k];
      ctx.lineWidth = 1.6 * dpr;
      ctx.stroke();
    });
  }
}

function sup(e) {
  const map = { "-": "⁻", 0: "⁰", 1: "¹", 2: "²", 3: "³" };
  return String(e)
    .split("")
    .map((c) => map[c] ?? c)
    .join("");
}
