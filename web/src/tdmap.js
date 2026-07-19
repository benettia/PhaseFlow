// Taitel-Dukler map: classified background grid (log-log jg/jl), live cell dots.

const REGIME_COLORS = [
  "#9db4c8", "#7d9cb8", "#5e7fa3", "#e0bd6f", "#37629c",
  "#4a6f9e", "#8a86c8", "#284b74", "#efd9a4",
];
const INK = "#1c1b18", INK2 = "#6b665c";

export class TDMap {
  constructor(canvas, classifyPoint) {
    this.cv = canvas;
    this.ctx = canvas.getContext("2d");
    this.classify = classifyPoint;
    this.jgRange = [-2, 1.7]; // log10 m/s
    this.jlRange = [-2.3, 1];
    this.bg = null;
    this.bgKey = "";
  }

  buildBackground(d, p) {
    const key = `${d.toFixed(4)}|${(p / 1e4).toFixed(0)}`;
    if (key === this.bgKey) return;
    this.bgKey = key;
    const W = 66, H = 52;
    const off = new OffscreenCanvas(W, H);
    const octx = off.getContext("2d");
    const img = octx.createImageData(W, H);
    for (let iy = 0; iy < H; iy++) {
      for (let ix = 0; ix < W; ix++) {
        const jg = 10 ** (this.jgRange[0] + (ix / (W - 1)) * (this.jgRange[1] - this.jgRange[0]));
        const jl = 10 ** (this.jlRange[1] - (iy / (H - 1)) * (this.jlRange[1] - this.jlRange[0]));
        const r = this.classify(jg, jl, d, 0, 1, p);
        const c = REGIME_COLORS[r] || "#999";
        const k = 4 * (iy * W + ix);
        img.data[k] = parseInt(c.slice(1, 3), 16);
        img.data[k + 1] = parseInt(c.slice(3, 5), 16);
        img.data[k + 2] = parseInt(c.slice(5, 7), 16);
        img.data[k + 3] = 105;
      }
    }
    octx.putImageData(img, 0, 0);
    this.bg = off;
  }

  draw(sim) {
    const { ctx, cv } = this;
    const w = cv.width, h = cv.height;
    const L = 44, B = 30, T = 12, R = 10;
    ctx.clearRect(0, 0, w, h);
    if (sim) {
      const d = sim.diam()[0];
      this.buildBackground(d, 2e5);
    }
    if (this.bg) {
      ctx.imageSmoothingEnabled = true;
      ctx.drawImage(this.bg, L, T, w - L - R, h - T - B);
    }
    ctx.strokeStyle = INK;
    ctx.lineWidth = 1.2;
    ctx.strokeRect(L + 0.5, T + 0.5, w - L - R - 1, h - T - B - 1);
    ctx.fillStyle = INK2;
    ctx.font = "10.5px ui-monospace, monospace";
    for (let e = Math.ceil(this.jgRange[0]); e <= this.jgRange[1]; e++) {
      const x = L + ((e - this.jgRange[0]) / (this.jgRange[1] - this.jgRange[0])) * (w - L - R);
      ctx.fillText(`1e${e}`, x - 10, h - B + 14);
    }
    for (let e = Math.ceil(this.jlRange[0]); e <= this.jlRange[1]; e++) {
      const y = T + ((this.jlRange[1] - e) / (this.jlRange[1] - this.jlRange[0])) * (h - T - B);
      ctx.fillText(`1e${e}`, 6, y + 3);
    }
    ctx.fillStyle = INK;
    ctx.fillText("j_gas  [m/s]  — Taitel–Dukler map, live cells", L, h - 6);
    if (!sim) return;
    const n = sim.n_cells();
    const alpha = sim.alpha(), vg = sim.vg(), vl = sim.vl();
    ctx.fillStyle = "rgba(28,27,24,0.75)";
    for (let i = 0; i < n; i += 2) {
      const jg = Math.abs(alpha[i] * vg[i]), jl = Math.abs((1 - alpha[i]) * vl[i]);
      if (jg < 1e-4 || jl < 1e-4) continue;
      const x = L + ((Math.log10(jg) - this.jgRange[0]) / (this.jgRange[1] - this.jgRange[0])) * (w - L - R);
      const y = T + ((this.jlRange[1] - Math.log10(jl)) / (this.jlRange[1] - this.jlRange[0])) * (h - T - B);
      if (x < L || x > w - R || y < T || y > h - B) continue;
      ctx.beginPath();
      ctx.arc(x, y, 1.6, 0, 7);
      ctx.fill();
    }
  }
}
