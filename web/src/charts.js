// Scrolling strip charts: engineering-recorder style, ring buffer per series.

const INK = "#1c1b18", INK2 = "#6b665c", GRID = "#ddd5c2";

export class StripChart {
  constructor(canvas, title, unit, colors, scale = 1) {
    this.cv = canvas;
    this.ctx = canvas.getContext("2d");
    this.title = title;
    this.unit = unit;
    this.colors = colors;
    this.scale = scale;
    this.cap = 700;
    this.buf = []; // [t, v0, v1, v2]
  }

  reset() {
    this.buf = [];
  }

  push(t, values) {
    this.buf.push([t, ...values]);
    if (this.buf.length > this.cap) this.buf.shift();
  }

  draw() {
    const { ctx, cv } = this;
    const w = cv.width, h = cv.height;
    ctx.clearRect(0, 0, w, h);
    ctx.strokeStyle = INK;
    ctx.lineWidth = 1;
    ctx.strokeRect(0.5, 0.5, w - 1, h - 1);
    ctx.font = "11px ui-monospace, monospace";
    if (this.buf.length < 2) {
      ctx.fillStyle = INK2;
      ctx.fillText(this.title, 8, 15);
      return;
    }
    const ns = this.buf[0].length - 1;
    let lo = Infinity, hi = -Infinity;
    for (const row of this.buf)
      for (let s = 1; s <= ns; s++) {
        lo = Math.min(lo, row[s]);
        hi = Math.max(hi, row[s]);
      }
    const pad = 0.08 * (hi - lo || 1);
    lo -= pad; hi += pad;
    const t0 = this.buf[0][0], t1 = this.buf[this.buf.length - 1][0];
    const xat = (t) => 40 + ((t - t0) / Math.max(t1 - t0, 1e-9)) * (w - 48);
    const yat = (v) => h - 18 - ((v - lo) / (hi - lo)) * (h - 34);
    // gridlines
    ctx.strokeStyle = GRID;
    for (let g = 1; g < 4; g++) {
      const y = 16 + ((h - 34) * g) / 4;
      ctx.beginPath(); ctx.moveTo(40, y); ctx.lineTo(w - 8, y); ctx.stroke();
    }
    for (let s = 1; s <= ns; s++) {
      ctx.strokeStyle = this.colors[s - 1];
      ctx.lineWidth = 1.3;
      ctx.beginPath();
      for (let i = 0; i < this.buf.length; i++) {
        const px = xat(this.buf[i][0]), py = yat(this.buf[i][s]);
        i ? ctx.lineTo(px, py) : ctx.moveTo(px, py);
      }
      ctx.stroke();
    }
    ctx.fillStyle = INK;
    ctx.fillText(this.title, 8, 13);
    ctx.fillStyle = INK2;
    ctx.fillText((hi * this.scale).toPrecision(4) + this.unit, 8, 26);
    ctx.fillText((lo * this.scale).toPrecision(4) + this.unit, 8, h - 6);
    ctx.fillText(`${(t1 - t0).toFixed(0)} s window`, w - 100, h - 6);
  }
}
