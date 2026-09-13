// Profiles along the pipe: stacked small multiples sharing one distance axis.
//
// This is the view you size a line from — where the pressure is spent, where
// liquid collects, how fast it is moving, how cold it gets. Everything is
// read from the same frame the pipe view is drawing, so the scrubber moves
// them together.

import { T, rgba, setFont } from "./theme.js";

const PAD_L = 48;
const PAD_R = 12;
const PAD_T = 16;
const PAD_B = 26;
const GAP = 12;

/// Panel definitions. `values` pulls a Float64Array-like series out of a view.
const PANELS = {
  pressure: {
    label: "pressure",
    unit: "bar",
    decimals: 2,
    // no band: a pressure trace sitting near the top of its own range would
    // shade the whole panel and read as a solid block
    series: [{ name: "p", color: T.ink, of: (v) => v.p, scale: 1e-5 }],
  },
  holdup: {
    label: "liquid holdup",
    unit: "",
    decimals: 3,
    fixed: [0, 1],
    series: [{ name: "H_l", color: T.liq, of: (v) => v.alpha, map: (a) => 1 - a }],
    band: true,
  },
  velocity: {
    label: "phase velocity",
    unit: "m/s",
    decimals: 2,
    zero: true,
    series: [
      { name: "v_gas", color: T.gasDeep, of: (v) => v.vg },
      { name: "v_liq", color: T.liq, of: (v) => v.vl },
    ],
  },
  temperature: {
    label: "temperature",
    unit: "°C",
    decimals: 1,
    series: [{ name: "T", color: T.warn, of: (v) => v.t, offset: -273.15 }],
  },
};

export class ProfileView {
  constructor(canvas) {
    this.cv = canvas;
    this.ctx = canvas.getContext("2d");
    this.keys = ["pressure", "holdup", "velocity"];
    this.cursor = null; // pointer x in metres
    canvas.addEventListener("pointermove", (e) => {
      const r = canvas.getBoundingClientRect();
      this.px = ((e.clientX - r.left) * canvas.width) / r.width;
      this.redraw?.();
    });
    canvas.addEventListener("pointerleave", () => {
      this.px = null;
      this.redraw?.();
    });
  }

  /// Which panels to stack. Temperature only earns its space when the energy
  /// equation is actually running.
  setPanels(keys) {
    this.keys = keys.filter((k) => PANELS[k]);
  }

  draw(view, geom) {
    const { ctx, cv } = this;
    const dpr = devicePixelRatio;
    ctx.clearRect(0, 0, cv.width, cv.height);
    ctx.fillStyle = T.cream;
    ctx.fillRect(0, 0, cv.width, cv.height);
    if (!view || !geom?.x?.length) return;

    const x = geom.x;
    const n = Math.min(view.n, x.length);
    const xMax = geom.total || x[n - 1];
    const L = PAD_L * dpr;
    const R = cv.width - PAD_R * dpr;
    const plotW = R - L;
    const nP = this.keys.length;
    const totalH = cv.height - (PAD_T + PAD_B) * dpr - (nP - 1) * GAP * dpr;
    const panelH = totalH / nP;
    if (plotW <= 10 || panelH <= 10) return;

    const xat = (m) => L + (m / Math.max(xMax, 1e-9)) * plotW;
    const cursorM = this.px == null ? null : ((this.px - L) / plotW) * xMax;
    const ci =
      cursorM == null || cursorM < 0 || cursorM > xMax
        ? null
        : Math.max(0, Math.min(n - 1, Math.round((cursorM / xMax) * (n - 1))));

    this.keys.forEach((key, k) => {
      const pane = PANELS[key];
      const top = PAD_T * dpr + k * (panelH + GAP * dpr);
      this.drawPanel(pane, view, n, xat, top, panelH, L, plotW, dpr, ci);
    });

    // shared distance axis
    const axisY = PAD_T * dpr + nP * panelH + (nP - 1) * GAP * dpr;
    ctx.strokeStyle = T.line;
    ctx.lineWidth = 1.1 * dpr;
    ctx.beginPath();
    ctx.moveTo(L, axisY);
    ctx.lineTo(R, axisY);
    ctx.stroke();
    setFont(ctx, 9.5 * dpr);
    ctx.fillStyle = T.ink3;
    ctx.textAlign = "center";
    const step = niceStep(xMax, 6);
    for (let m = 0; m <= xMax + 1e-9; m += step) {
      const px = xat(m);
      ctx.beginPath();
      ctx.moveTo(px, axisY);
      ctx.lineTo(px, axisY + 3 * dpr);
      ctx.stroke();
      ctx.fillText(fmtLen(m), px, axisY + 14 * dpr);
    }
    ctx.fillStyle = T.ink2;
    ctx.fillText("distance along pipe [m]", (L + R) / 2, cv.height - 3 * dpr);
    ctx.textAlign = "left";

    // segment boundaries, so elbows are visible in every panel
    if (geom.breaks?.length) {
      ctx.strokeStyle = rgba(T.ink3, 0.5);
      ctx.setLineDash([3 * dpr, 3 * dpr]);
      ctx.lineWidth = 1;
      for (const b of geom.breaks) {
        const px = xat(b);
        ctx.beginPath();
        ctx.moveTo(px, PAD_T * dpr);
        ctx.lineTo(px, axisY);
        ctx.stroke();
      }
      ctx.setLineDash([]);
    }

    // cursor
    if (ci != null) {
      ctx.strokeStyle = rgba(T.warn, 0.75);
      ctx.lineWidth = 1 * dpr;
      ctx.beginPath();
      ctx.moveTo(xat(x[ci]), PAD_T * dpr);
      ctx.lineTo(xat(x[ci]), axisY);
      ctx.stroke();
      setFont(ctx, 9.5 * dpr, "600");
      ctx.fillStyle = T.warn;
      ctx.textAlign = "center";
      ctx.fillText(`${x[ci].toFixed(1)} m`, xat(x[ci]), PAD_T * dpr - 4 * dpr);
      ctx.textAlign = "left";
    }
  }

  drawPanel(pane, view, n, xat, top, h, L, plotW, dpr, ci) {
    const { ctx } = this;
    const bottom = top + h;
    // range
    let lo = Infinity;
    let hi = -Infinity;
    const cols = pane.series.map((s) => {
      const raw = s.of(view);
      const out = new Float64Array(n);
      for (let i = 0; i < n; i++) {
        let v = raw[i];
        if (s.map) v = s.map(v);
        if (s.scale) v *= s.scale;
        if (s.offset) v += s.offset;
        out[i] = v;
        if (v < lo) lo = v;
        if (v > hi) hi = v;
      }
      return out;
    });
    if (pane.fixed) {
      [lo, hi] = pane.fixed;
    } else {
      if (pane.zero) {
        lo = Math.min(lo, 0);
        hi = Math.max(hi, 0);
      }
      if (!(hi > lo)) {
        hi = lo + 1;
        lo -= 1;
      }
      const pad = 0.08 * (hi - lo);
      lo -= pad;
      hi += pad;
    }
    const yat = (v) => bottom - ((v - lo) / (hi - lo)) * h;

    // grid + ticks. Tick count follows the panel height: four labels in a
    // 40 px panel is a smear, not an axis.
    ctx.strokeStyle = T.grid;
    ctx.lineWidth = 1;
    setFont(ctx, 9.5 * dpr);
    ctx.textAlign = "right";
    const nTicks = Math.max(2, Math.min(4, Math.floor(h / (24 * dpr))));
    let step = niceStep(hi - lo, nTicks);
    // niceStep rounds the *step*, so the label count can still overshoot the
    // panel; coarsen until it fits
    while ((hi - lo) / step > nTicks + 1) step *= 2;
    const dec = stepDecimals(step);
    for (let v = Math.ceil(lo / step) * step; v <= hi + 1e-12; v += step) {
      const y = yat(v);
      if (y < top + 11 * dpr || y > bottom - 2 * dpr) continue;
      ctx.beginPath();
      ctx.moveTo(L, y);
      ctx.lineTo(L + plotW, y);
      ctx.stroke();
      ctx.fillStyle = T.ink3;
      ctx.fillText(axisNum(v, dec), L - 6 * dpr, y + 3.2 * dpr);
    }
    ctx.textAlign = "left";

    // traces
    cols.forEach((col, si) => {
      const s = pane.series[si];
      if (pane.band) {
        ctx.beginPath();
        for (let i = 0; i < n; i++) {
          const px = xat(view.xs ? view.xs[i] : i);
          const py = yat(col[i]);
          i ? ctx.lineTo(px, py) : ctx.moveTo(px, py);
        }
        ctx.lineTo(xat(view.xs[n - 1]), bottom);
        ctx.lineTo(xat(view.xs[0]), bottom);
        ctx.closePath();
        ctx.fillStyle = rgba(s.color, 0.1);
        ctx.fill();
      }
      ctx.beginPath();
      for (let i = 0; i < n; i++) {
        const px = xat(view.xs[i]);
        const py = yat(col[i]);
        i ? ctx.lineTo(px, py) : ctx.moveTo(px, py);
      }
      ctx.strokeStyle = s.color;
      ctx.lineWidth = 1.5 * dpr;
      ctx.stroke();
    });

    // frame + title + readout
    ctx.strokeStyle = T.line;
    ctx.lineWidth = 1.1 * dpr;
    ctx.strokeRect(L, top, plotW, h);
    setFont(ctx, 10 * dpr, "600");
    const unit = pane.unit ? ` [${pane.unit}]` : "";
    label(ctx, pane.label + unit, L + 5 * dpr, top + 2 * dpr, T.ink, dpr);

    let tx = L + plotW - 5 * dpr;
    ctx.textAlign = "right";
    for (let si = pane.series.length - 1; si >= 0; si--) {
      const s = pane.series[si];
      const val = ci == null ? cols[si][n - 1] : cols[si][ci];
      const text = `${s.name} ${trimNum(val, pane.decimals)}`;
      tx -= label(ctx, text, tx, top + 2 * dpr, s.color, dpr, "right");
    }
    ctx.textAlign = "left";

    if (ci != null) {
      cols.forEach((col, si) => {
        ctx.beginPath();
        ctx.arc(xat(view.xs[ci]), yat(col[ci]), 3 * dpr, 0, 7);
        ctx.fillStyle = T.cream;
        ctx.fill();
        ctx.strokeStyle = pane.series[si].color;
        ctx.lineWidth = 1.4 * dpr;
        ctx.stroke();
      });
    }
  }
}

/// Text on a cream plate, so a gridline never runs through a number.
/// Returns the width consumed (plus padding), for right-to-left packing.
function label(ctx, text, x, y, color, dpr, align = "left") {
  const w = ctx.measureText(text).width;
  const pad = 3 * dpr;
  ctx.fillStyle = T.cream;
  ctx.fillRect(align === "right" ? x - w - pad : x - pad, y, w + 2 * pad, 12 * dpr);
  ctx.fillStyle = color;
  ctx.textAlign = align;
  ctx.fillText(text, x, y + 9.5 * dpr);
  ctx.textAlign = "left";
  return w + 8 * dpr;
}

function stepDecimals(step) {
  if (step >= 1) return 0;
  return Math.min(4, Math.ceil(-Math.log10(step)));
}

function axisNum(v, dec) {
  const a = Math.abs(v);
  if (a >= 1e5) return v.toExponential(0);
  return v.toFixed(dec);
}

function niceStep(span, target) {
  const raw = span / Math.max(target, 1);
  const p = 10 ** Math.floor(Math.log10(raw || 1));
  const m = raw / p;
  return (m >= 5 ? 5 : m >= 2 ? 2 : 1) * p;
}

function trimNum(v, d) {
  const a = Math.abs(v);
  if (a >= 1e4) return v.toExponential(1);
  if (a >= 100) return v.toFixed(Math.min(d, 1));
  return v.toFixed(d);
}

function fmtLen(m) {
  return m >= 1000 ? `${(m / 1000).toFixed(1)}k` : `${m.toFixed(m < 10 ? 1 : 0)}`;
}
