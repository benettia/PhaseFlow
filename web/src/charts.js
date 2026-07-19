// Scrolling strip charts, engineering-recorder style. Data comes straight
// from the history buffer, so the charts and the pipe view can never
// disagree about what instant you are looking at — scrubbing moves both.

import { T, rgba, setFont } from "./theme.js";

export class StripChart {
  constructor(canvas, opts) {
    this.cv = canvas;
    this.ctx = canvas.getContext("2d");
    this.title = opts.title;
    this.unit = opts.unit ?? "";
    this.scale = opts.scale ?? 1;
    this.field = opts.field;
    this.map = opts.map ?? ((v) => v);
    this.decimals = opts.decimals ?? 1;
  }

  draw(history, probes, playhead) {
    const { ctx, cv } = this;
    const dpr = devicePixelRatio;
    const w = cv.width;
    const h = cv.height;
    ctx.clearRect(0, 0, w, h);
    ctx.fillStyle = T.cream;
    ctx.fillRect(0, 0, w, h);

    const L = 52 * dpr;
    const R = 58 * dpr;
    const TOP = 22 * dpr;
    const BOT = 20 * dpr;
    const plotW = w - L - R;
    const plotH = h - TOP - BOT;

    setFont(ctx, 11 * dpr, "600");
    ctx.fillStyle = T.ink;
    ctx.fillText(this.title, 8 * dpr, 14 * dpr);

    const m = history.length;
    if (m < 2 || plotW <= 0 || plotH <= 0) {
      setFont(ctx, 10.5 * dpr);
      ctx.fillStyle = T.ink3;
      ctx.fillText("waiting for data…", L, TOP + plotH / 2);
      return;
    }

    // gather series
    const series = probes.map((cell) => {
      const s = history.series(cell, this.field);
      for (let i = 0; i < s.v.length; i++) s.v[i] = this.map(s.v[i]) * this.scale;
      return s;
    });
    let lo = Infinity;
    let hi = -Infinity;
    for (const s of series) {
      for (const v of s.v) {
        if (v < lo) lo = v;
        if (v > hi) hi = v;
      }
    }
    if (!(hi > lo)) {
      hi = lo + 1;
      lo -= 1;
    }
    const pad = 0.1 * (hi - lo);
    lo -= pad;
    hi += pad;
    const t0 = history.at(0).t;
    const t1 = history.last().t;
    const xat = (t) => L + ((t - t0) / Math.max(t1 - t0, 1e-9)) * plotW;
    const yat = (v) => TOP + plotH - ((v - lo) / (hi - lo)) * plotH;

    // grid
    ctx.strokeStyle = T.grid;
    ctx.lineWidth = 1;
    setFont(ctx, 10 * dpr);
    ctx.fillStyle = T.ink3;
    ctx.textAlign = "right";
    const step = niceStep(hi - lo, 3);
    for (let v = Math.ceil(lo / step) * step; v <= hi; v += step) {
      const y = yat(v);
      ctx.beginPath();
      ctx.moveTo(L, y);
      ctx.lineTo(L + plotW, y);
      ctx.stroke();
      ctx.fillText(fmt(v, this.decimals), L - 6 * dpr, y + 3.5 * dpr);
    }
    ctx.textAlign = "left";

    // time ticks
    const tstep = niceStep(Math.max(t1 - t0, 1e-6), 4);
    ctx.textAlign = "center";
    for (let t = Math.ceil(t0 / tstep) * tstep; t <= t1; t += tstep) {
      const x = xat(t);
      ctx.strokeStyle = T.grid;
      ctx.beginPath();
      ctx.moveTo(x, TOP);
      ctx.lineTo(x, TOP + plotH);
      ctx.stroke();
      ctx.fillStyle = T.ink3;
      ctx.fillText(`${t.toFixed(t1 - t0 > 20 ? 0 : 1)}s`, x, h - 6 * dpr);
    }
    ctx.textAlign = "left";

    // frame
    ctx.strokeStyle = T.line;
    ctx.lineWidth = 1.1 * dpr;
    ctx.strokeRect(L, TOP, plotW, plotH);

    // traces
    series.forEach((s, k) => {
      const color = T.probe[k];
      ctx.beginPath();
      for (let i = 0; i < s.t.length; i++) {
        const x = xat(s.t[i]);
        const y = yat(s.v[i]);
        i ? ctx.lineTo(x, y) : ctx.moveTo(x, y);
      }
      ctx.lineTo(xat(s.t[s.t.length - 1]), TOP + plotH);
      ctx.lineTo(xat(s.t[0]), TOP + plotH);
      ctx.closePath();
      ctx.fillStyle = rgba(color, 0.06);
      ctx.fill();

      ctx.beginPath();
      for (let i = 0; i < s.t.length; i++) {
        const x = xat(s.t[i]);
        const y = yat(s.v[i]);
        i ? ctx.lineTo(x, y) : ctx.moveTo(x, y);
      }
      ctx.strokeStyle = color;
      ctx.lineWidth = 1.4 * dpr;
      ctx.stroke();
    });

    // playhead + readouts
    const idx = Math.max(0, Math.min(m - 1, playhead ?? m - 1));
    const px = xat(series[0].t[idx]);
    ctx.strokeStyle = rgba(T.ink, 0.55);
    ctx.lineWidth = 1 * dpr;
    ctx.setLineDash([3 * dpr, 3 * dpr]);
    ctx.beginPath();
    ctx.moveTo(px, TOP);
    ctx.lineTo(px, TOP + plotH);
    ctx.stroke();
    ctx.setLineDash([]);

    setFont(ctx, 10.5 * dpr, "600");
    series.forEach((s, k) => {
      const y = yat(s.v[idx]);
      ctx.fillStyle = T.cream;
      ctx.strokeStyle = T.probe[k];
      ctx.lineWidth = 1.4 * dpr;
      ctx.beginPath();
      ctx.arc(px, y, 3.2 * dpr, 0, 7);
      ctx.fill();
      ctx.stroke();
      ctx.fillStyle = T.probe[k];
      ctx.fillText(
        `${k + 1} ${fmt(s.v[idx], this.decimals)}${this.unit}`,
        L + plotW + 7 * dpr,
        TOP + 11 * dpr + k * 14 * dpr,
      );
    });
  }
}

function fmt(v, d) {
  const a = Math.abs(v);
  if (a >= 1000) return v.toFixed(0);
  if (a >= 100) return v.toFixed(Math.min(d, 1));
  return v.toFixed(d);
}

function niceStep(span, target) {
  const raw = span / Math.max(target, 1);
  const p = 10 ** Math.floor(Math.log10(raw || 1));
  const m = raw / p;
  return (m >= 5 ? 5 : m >= 2 ? 2 : 1) * p;
}
