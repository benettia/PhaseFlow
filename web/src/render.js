// Pipe view: the pipe drawn along its true geometry, every cell filled by
// its flow regime. Also the polyline editor (drag / split / delete vertices).
//
// Cells are drawn as quads that share their edges exactly, so the pipe reads
// as one continuous vessel rather than a row of tiles. Detail art is clipped
// to each quad and advected by the local phase velocity.

import { REGIME_COLORS, T, rgba, setFont } from "./theme.js";

const MIN_W = 11;
const MAX_W = 44;

export class PipeView {
  constructor(canvas, onEdit) {
    this.cv = canvas;
    this.ctx = canvas.getContext("2d");
    this.onEdit = onEdit;
    this.verts = []; // polyline vertices in metres {x, z}
    this.diams = []; // per segment
    this.segs = []; // scenario segments (authoritative cell counts)
    this.probes = [];
    this.drag = -1;
    this.hover = -1;
    canvas.addEventListener("pointerdown", (e) => this.pointerDown(e));
    canvas.addEventListener("pointermove", (e) => this.pointerMove(e));
    canvas.addEventListener("pointerup", () => this.pointerUp());
    canvas.addEventListener("pointerleave", () => {
      this.hover = -1;
    });
    canvas.addEventListener("dblclick", (e) => this.split(e));
  }

  setScenario(sc) {
    this.verts = [{ x: 0, z: 0 }];
    this.diams = [];
    this.segs = sc.segments.map((s) => ({ ...s }));
    let x = 0;
    let z = 0;
    for (const s of sc.segments) {
      const th = (s.angle * Math.PI) / 180;
      x += s.length * Math.cos(th);
      z += s.length * Math.sin(th);
      this.verts.push({ x, z });
      this.diams.push(s.diameter);
    }
    this.buildCells();
    this.fit();
  }

  setProbes(p) {
    this.probes = p;
  }

  /// Arc-length of every cell face + per-cell diameter, straight from the
  /// scenario's own cell counts (so it matches the solver exactly).
  buildCells() {
    this.faceS = [0];
    this.cellDiam = [];
    this.segRange = [];
    let s = 0;
    for (const seg of this.segs) {
      const d = seg.length / seg.cells;
      const start = this.cellDiam.length;
      for (let k = 0; k < seg.cells; k++) {
        s += d;
        this.faceS.push(s);
        this.cellDiam.push(seg.diameter);
      }
      this.segRange.push([start, this.cellDiam.length]);
    }
    this.totalLen = s;
    this.dmax = Math.max(...this.cellDiam);
  }

  /// vertices -> scenario segments (cells redistributed, ~240 total)
  segments() {
    const lens = [];
    let total = 0;
    for (let i = 0; i + 1 < this.verts.length; i++) {
      const dx = this.verts[i + 1].x - this.verts[i].x;
      const dz = this.verts[i + 1].z - this.verts[i].z;
      lens.push(Math.hypot(dx, dz));
      total += lens[i];
    }
    return lens.map((len, i) => {
      const dx = this.verts[i + 1].x - this.verts[i].x;
      const dz = this.verts[i + 1].z - this.verts[i].z;
      return {
        length: Math.max(len, 0.5),
        angle: (Math.atan2(dz, dx) * 180) / Math.PI,
        diameter: this.diams[i],
        cells: Math.max(6, Math.round((240 * len) / total)),
      };
    });
  }

  fit() {
    const w = this.cv.width;
    const h = this.cv.height;
    let x0 = Infinity;
    let x1 = -Infinity;
    let z0 = Infinity;
    let z1 = -Infinity;
    for (const v of this.verts) {
      x0 = Math.min(x0, v.x);
      x1 = Math.max(x1, v.x);
      z0 = Math.min(z0, v.z);
      z1 = Math.max(z1, v.z);
    }
    const padX = 62 * devicePixelRatio;
    const padY = 46 * devicePixelRatio;
    const sx = (w - 2 * padX) / Math.max(x1 - x0, 1e-6);
    const sz = (h - 2 * padY) / Math.max(z1 - z0, 1e-6);
    this.scale = Math.min(sx, sz, 1e4);
    this.ox = padX - x0 * this.scale + (w - 2 * padX - (x1 - x0) * this.scale) / 2;
    this.oz = h - padY + z0 * this.scale - (h - 2 * padY - (z1 - z0) * this.scale) / 2;
    this.buildScreenPath();
  }

  toScreen(x, z) {
    return [this.ox + x * this.scale, this.oz - z * this.scale];
  }

  fromScreen(px, pz) {
    return { x: (px - this.ox) / this.scale, z: (this.oz - pz) / this.scale };
  }

  /// Screen-space polyline + per-segment cumulative arc length (metres).
  buildScreenPath() {
    this.pts = this.verts.map((v) => this.toScreen(v.x, v.z));
    this.segLen = [];
    this.segDir = [];
    this.cum = [0];
    for (let i = 0; i + 1 < this.verts.length; i++) {
      const a = this.verts[i];
      const b = this.verts[i + 1];
      const len = Math.max(Math.hypot(b.x - a.x, b.z - a.z), 1e-9);
      this.segLen.push(len);
      const [ax, az] = this.pts[i];
      const [bx, bz] = this.pts[i + 1];
      const d = Math.hypot(bx - ax, bz - az) || 1;
      this.segDir.push([(bx - ax) / d, (bz - az) / d]);
      this.cum.push(this.cum[i] + len);
    }
  }

  /// Screen point + segment index at arc length s (metres).
  pointAt(s) {
    let i = 0;
    while (i < this.segLen.length - 1 && s > this.cum[i + 1]) i++;
    const t = Math.min(1, Math.max(0, (s - this.cum[i]) / this.segLen[i]));
    const [ax, az] = this.pts[i];
    const [bx, bz] = this.pts[i + 1];
    return [ax + t * (bx - ax), az + t * (bz - az), i, t];
  }

  widthPx(d) {
    const f = this.dmax > 0 ? Math.sqrt(d / this.dmax) : 1;
    // scale with the viewport so the pipe stays a vessel, not a wire
    const base = Math.max(20 * devicePixelRatio, Math.min(MAX_W * devicePixelRatio, this.cv.height * 0.062));
    return Math.max(MIN_W * devicePixelRatio, base * f);
  }

  /// Cell quad, its far edge pushed forward by `inflate` px.
  ///
  /// Two abutting anti-aliased fills leave a pale seam down the shared edge,
  /// which reads as tick marks along the pipe — so cells must overlap. The
  /// overlap is deliberately one-sided: cells are drawn in order, so a
  /// symmetric overlap would let each cell repaint its neighbour's near end
  /// (a gas sliver punched through the liquid wherever holdup steps between
  /// cells). Extending only forward means a cell never intrudes on
  /// territory already drawn.
  quadPath(ctx, w, i, inflate) {
    const dx = w.mid[i + 1][0] - w.mid[i][0];
    const dz = w.mid[i + 1][1] - w.mid[i][1];
    const d = Math.hypot(dx, dz) || 1;
    const ex = (dx / d) * inflate;
    const ez = (dz / d) * inflate;
    ctx.beginPath();
    ctx.moveTo(w.lo[i][0], w.lo[i][1]);
    ctx.lineTo(w.lo[i + 1][0] + ex, w.lo[i + 1][1] + ez);
    ctx.lineTo(w.hi[i + 1][0] + ex, w.hi[i + 1][1] + ez);
    ctx.lineTo(w.hi[i][0], w.hi[i][1]);
    ctx.closePath();
  }

  /// Wall points for every face: shared by neighbouring cells, so no seams.
  buildWalls(n) {
    const lo = new Array(n + 1);
    const hi = new Array(n + 1);
    const mid = new Array(n + 1);
    for (let j = 0; j <= n; j++) {
      const [px, pz, si, t] = this.pointAt(this.faceS[j]);
      // direction: the segment's own, or the bisector at a junction
      let [dx, dz] = this.segDir[si];
      const atStart = t <= 1e-6 && si > 0;
      const atEnd = t >= 1 - 1e-6 && si < this.segDir.length - 1;
      let miter = 1;
      if (atStart || atEnd) {
        const other = this.segDir[atStart ? si - 1 : si + 1];
        const bx = dx + other[0];
        const bz = dz + other[1];
        const bl = Math.hypot(bx, bz) || 1;
        const nx = bx / bl;
        const nz = bz / bl;
        // clamped: an unbounded miter spikes the outer corner of a sharp
        // elbow into a wedge that no cell's art covers cleanly
        miter = Math.min(1.12, 1 / Math.max(0.4, nx * dx + nz * dz));
        dx = nx;
        dz = nz;
      }
      const d = this.cellDiam[Math.min(n - 1, j)];
      const h = 0.5 * this.widthPx(d) * miter;
      lo[j] = [px - dz * h, pz + dx * h];
      hi[j] = [px + dz * h, pz - dx * h];
      mid[j] = [px, pz];
    }
    return { lo, hi, mid };
  }

  // ---------- drawing ----------

  draw(view) {
    const { ctx, cv } = this;
    ctx.clearRect(0, 0, cv.width, cv.height);
    ctx.lineJoin = "round";
    ctx.lineCap = "round";
    if (!view || !this.pts) return;
    const n = view.n;
    if (n !== this.cellDiam.length) return; // geometry mid-swap
    const dpr = devicePixelRatio;
    const w = this.buildWalls(n);

    this.drawElevationGrid(ctx, cv);

    // 1. the whole interior is gas to begin with — one polygon, no seams
    ctx.beginPath();
    ctx.moveTo(w.lo[0][0], w.lo[0][1]);
    for (let j = 1; j <= n; j++) ctx.lineTo(w.lo[j][0], w.lo[j][1]);
    for (let j = n; j >= 0; j--) ctx.lineTo(w.hi[j][0], w.hi[j][1]);
    ctx.closePath();
    ctx.fillStyle = T.gas;
    ctx.fill();

    // 2. stratified runs: one continuous liquid body with a real surface
    this.drawStratified(ctx, w, view, n, dpr);

    // 3. non-stratified cells: liquid base + regime art, clipped per cell
    for (let i = 0; i < n; i++) {
      const reg = view.regime[i];
      if (reg <= 1) continue; // stratified runs are drawn as one surface below
      const cx = (w.lo[i][0] + w.lo[i + 1][0] + w.hi[i][0] + w.hi[i + 1][0]) / 4;
      const cz = (w.lo[i][1] + w.lo[i + 1][1] + w.hi[i][1] + w.hi[i + 1][1]) / 4;
      const dx = w.mid[i + 1][0] - w.mid[i][0];
      const dz = w.mid[i + 1][1] - w.mid[i][1];
      const ds = Math.hypot(dx, dz) || 1;
      const ang = Math.atan2(dz, dx);
      const width = Math.max(
        Math.hypot(w.lo[i][0] - w.hi[i][0], w.lo[i][1] - w.hi[i][1]),
        Math.hypot(w.lo[i + 1][0] - w.hi[i + 1][0], w.lo[i + 1][1] - w.hi[i + 1][1]),
      );
      ctx.save();
      this.quadPath(ctx, w, i, 1.0 * dpr);
      ctx.clip();
      ctx.translate(cx, cz);
      ctx.rotate(ang);
      const sPix = 0.5 * (this.faceS[i] + this.faceS[i + 1]) * this.scale;
      this.drawCell(ctx, ds, width, view, i, dpr, sPix);
      ctx.restore();
    }

    // 4. cylindrical shading — once per straight run, so no per-cell seams
    for (const [a, b] of this.segRange) {
      if (b <= a) continue;
      ctx.beginPath();
      ctx.moveTo(w.lo[a][0], w.lo[a][1]);
      for (let j = a + 1; j <= b; j++) ctx.lineTo(w.lo[j][0], w.lo[j][1]);
      for (let j = b; j >= a; j--) ctx.lineTo(w.hi[j][0], w.hi[j][1]);
      ctx.closePath();
      const m = (a + b) >> 1;
      const g = ctx.createLinearGradient(w.hi[m][0], w.hi[m][1], w.lo[m][0], w.lo[m][1]);
      g.addColorStop(0, "rgba(255,255,255,0.22)");
      g.addColorStop(0.45, "rgba(255,255,255,0.02)");
      g.addColorStop(1, "rgba(0,0,0,0.17)");
      ctx.fillStyle = g;
      ctx.fill();
    }

    // 5. walls
    ctx.strokeStyle = T.ink;
    ctx.lineWidth = 1.3 * dpr;
    for (const side of [w.lo, w.hi]) {
      ctx.beginPath();
      side.forEach(([x, y], j) => (j ? ctx.lineTo(x, y) : ctx.moveTo(x, y)));
      ctx.stroke();
    }
    ctx.beginPath();
    ctx.moveTo(w.lo[0][0], w.lo[0][1]);
    ctx.lineTo(w.hi[0][0], w.hi[0][1]);
    ctx.moveTo(w.lo[n][0], w.lo[n][1]);
    ctx.lineTo(w.hi[n][0], w.hi[n][1]);
    ctx.stroke();

    this.drawProbes(ctx, w, dpr);
    this.drawEnds(ctx, w, n, dpr);
    this.drawHandles(ctx, dpr);
    this.drawScale(ctx, cv, dpr);
  }

  /// Regime art for one cell. Every pattern is phased on the cell's absolute
  /// position along the pipe (sPix) rather than its index, so waves, Taylor
  /// bubbles and bubble trains stay continuous across cell boundaries and
  /// actually travel — per-cell phases made the pipe look like a barcode.
  drawCell(ctx, ds, width, view, i, dpr, sPix) {
    const a = view.alpha[i];
    const reg = view.regime[i];
    const t = view.t;
    const vg = view.vg[i];
    const hold = 1 - a;
    const L = ds / 2 + 2 * dpr; // must exceed the clip inflation (0.75·dpr)
    const H = width / 2;
    const drift = vg * t * this.scale; // px travelled by the pattern

    switch (reg) {
      case 7: // single liquid
      case 8: // single gas
        ctx.fillStyle = reg === 7 ? T.liq : T.gas;
        ctx.fillRect(-L, -H, 2 * L, width);
        break;

      case 3: {
        // annular — gas core, liquid film rippling along the wall
        ctx.fillStyle = T.gas;
        ctx.fillRect(-L, -H, 2 * L, width);
        const film = Math.max(1.2 * dpr, (hold * width) / 2);
        const amp = Math.min(1.2 * dpr, film * 0.45);
        const k = (2 * Math.PI) / Math.max(10 * dpr, width);
        ctx.fillStyle = T.liq;
        for (const side of [-1, 1]) {
          ctx.beginPath();
          ctx.moveTo(-L, side * H);
          for (let px = -L; px <= L; px += 2 * dpr) {
            ctx.lineTo(px, side * (H - film) + amp * Math.sin(k * (px + sPix - drift) + side));
          }
          ctx.lineTo(L, side * H);
          ctx.closePath();
          ctx.fill();
        }
        break;
      }

      case 2: {
        // intermittent — a Taylor-bubble / liquid-slug train riding the pipe
        ctx.fillStyle = T.liq;
        ctx.fillRect(-L, -H, 2 * L, width);
        const lambda = Math.max(4.5 * width, 30 * dpr);
        const blen = lambda * Math.min(0.82, 0.2 + 0.75 * a);
        const bh = width * (0.5 + 0.32 * a);
        ctx.fillStyle = T.gas;
        forEachOnLattice(sPix, drift, lambda, L, (xl) => capsule(ctx, xl, 0, blen, bh));
        break;
      }

      case 6: {
        // churn — broken, chaotic gas structures
        ctx.fillStyle = T.liq;
        ctx.fillRect(-L, -H, 2 * L, width);
        ctx.fillStyle = T.gasDeep;
        const spacing = Math.max(9 * dpr, width * 0.85);
        forEachOnLattice(sPix, drift, spacing, L, (xl, k) => {
          const py = (hash(k * 31) - 0.5) * width * 0.6;
          capsule(ctx, xl, py, width * (0.45 + 0.75 * hash(k * 7)), width * 0.32);
        });
        break;
      }

      default: {
        // bubbly / dispersed bubble — stipple density proportional to void
        ctx.fillStyle = T.liq;
        ctx.fillRect(-L, -H, 2 * L, width);
        ctx.fillStyle = T.gasLight;
        const spacing = Math.max(4.5 * dpr, width * (0.62 - 0.42 * a));
        forEachOnLattice(sPix, drift, spacing, L, (xl, k) => {
          const py = (hash(k * 17) - 0.5) * (width - 3.5 * dpr);
          const r = (0.9 + 1.4 * hash(k * 53)) * dpr;
          ctx.beginPath();
          ctx.arc(xl + (hash(k * 11) - 0.5) * spacing * 0.5, py, r, 0, 7);
          ctx.fill();
        });
      }
    }
  }

  /// Contiguous runs of stratified cells are filled as a single body whose
  /// top edge is the interface polyline — a real liquid level that steps and
  /// ripples along the pipe, instead of one rectangle per cell (which shows
  /// its seams the moment holdup varies between neighbours).
  drawStratified(ctx, w, view, n, dpr) {
    const holdAt = (face, a, b) => {
      const l = Math.max(a, Math.min(b - 1, face - 1));
      const r = Math.max(a, Math.min(b - 1, face));
      return 1 - 0.5 * (view.alpha[l] + view.alpha[r]);
    };
    let i = 0;
    while (i < n) {
      if (view.regime[i] > 1) {
        i++;
        continue;
      }
      let j = i;
      let wavy = false;
      while (j < n && view.regime[j] <= 1) {
        wavy = wavy || view.regime[j] === 1;
        j++;
      }
      // A run's terminal faces are mitered at an elbow, which leaves a
      // wedge of the bent end cell uncovered. Spill the body a few px past
      // each end: neighbouring cells are drawn afterwards and reclaim their
      // own territory, and a bend really does pool liquid in its corner.
      const ext = 2.5 * dpr;
      const endShift = (f) => {
        if (f !== i && f !== j) return [0, 0];
        const k = f === i ? i : j - 1;
        const dx = w.mid[k + 1][0] - w.mid[k][0];
        const dz = w.mid[k + 1][1] - w.mid[k][1];
        const d = Math.hypot(dx, dz) || 1;
        const sgn = f === i ? -1 : 1;
        return [(dx / d) * ext * sgn, (dz / d) * ext * sgn];
      };
      const surface = [];
      for (let f = i; f <= j; f++) {
        // gravity-lower wall point, and the interface a holdup-fraction up
        const down = w.lo[f][1] > w.hi[f][1] ? w.lo[f] : w.hi[f];
        const up = w.lo[f][1] > w.hi[f][1] ? w.hi[f] : w.lo[f];
        const hold = holdAt(f, i, j);
        let x = down[0] + (up[0] - down[0]) * hold;
        let y = down[1] + (up[1] - down[1]) * hold;
        if (wavy) {
          const cell = Math.max(i, Math.min(j - 1, f));
          const width = Math.hypot(up[0] - down[0], up[1] - down[1]);
          const amp = Math.min(2.4 * dpr, hold * width * 0.3);
          const k = (2 * Math.PI) / Math.max(16 * dpr, width * 1.8);
          const sPix = this.faceS[f] * this.scale;
          const drift = view.vg[cell] * view.t * this.scale;
          y += amp * Math.sin(k * (sPix - drift));
        }
        const [sx, sy] = endShift(f);
        surface.push([x + sx, y + sy]);
      }
      ctx.beginPath();
      surface.forEach(([x, y], k) => (k ? ctx.lineTo(x, y) : ctx.moveTo(x, y)));
      for (let f = j; f >= i; f--) {
        const down = w.lo[f][1] > w.hi[f][1] ? w.lo[f] : w.hi[f];
        const [sx, sy] = endShift(f);
        ctx.lineTo(down[0] + sx, down[1] + sy);
      }
      ctx.closePath();
      ctx.fillStyle = T.liq;
      ctx.fill();
      // the free surface itself
      ctx.beginPath();
      surface
        .slice(1, -1)
        .forEach(([x, y], k) => (k ? ctx.lineTo(x, y) : ctx.moveTo(x, y)));
      ctx.strokeStyle = rgba(T.liqLight, 0.9);
      ctx.lineWidth = 1.1 * dpr;
      ctx.stroke();
      i = j;
    }
  }

  drawElevationGrid(ctx, cv) {
    const zs = this.verts.map((v) => v.z);
    const zmin = Math.min(...zs);
    const zmax = Math.max(...zs);
    if (zmax - zmin < 1e-6) return;
    const step = niceStep(zmax - zmin, 4);
    ctx.strokeStyle = T.grid;
    ctx.lineWidth = 1;
    ctx.fillStyle = T.ink3;
    setFont(ctx, 10.5 * devicePixelRatio);
    ctx.textAlign = "left";
    for (let z = Math.ceil(zmin / step) * step; z <= zmax + 1e-9; z += step) {
      const [, y] = this.toScreen(0, z);
      ctx.beginPath();
      ctx.setLineDash([2 * devicePixelRatio, 4 * devicePixelRatio]);
      ctx.moveTo(8 * devicePixelRatio, y);
      ctx.lineTo(cv.width - 8 * devicePixelRatio, y);
      ctx.stroke();
      ctx.setLineDash([]);
      ctx.fillText(`${z.toFixed(0)} m`, 10 * devicePixelRatio, y - 4 * devicePixelRatio);
    }
  }

  drawProbes(ctx, w, dpr) {
    setFont(ctx, 10 * dpr, "600");
    ctx.textAlign = "center";
    this.probes.forEach((cell, k) => {
      if (cell == null || !w.mid[cell]) return;
      const [x, y] = w.mid[cell];
      const up = w.hi[cell];
      const dx = up[0] - x;
      const dy = up[1] - y;
      const d = Math.hypot(dx, dy) || 1;
      const px = x + (dx / d) * 20 * dpr;
      const py = y + (dy / d) * 20 * dpr;
      ctx.strokeStyle = T.probe[k];
      ctx.lineWidth = 1.2 * dpr;
      ctx.beginPath();
      ctx.moveTo(x + (dx / d) * 3 * dpr, y + (dy / d) * 3 * dpr);
      ctx.lineTo(px, py);
      ctx.stroke();
      ctx.fillStyle = T.cream;
      ctx.beginPath();
      ctx.arc(px, py, 7 * dpr, 0, 7);
      ctx.fill();
      ctx.stroke();
      ctx.fillStyle = T.probe[k];
      ctx.fillText(`${k + 1}`, px, py + 3.5 * dpr);
    });
    ctx.textAlign = "left";
  }

  drawEnds(ctx, w, n, dpr) {
    setFont(ctx, 10 * dpr);
    ctx.fillStyle = T.ink2;
    const [ix, iy] = w.mid[0];
    const [ox, oy] = w.mid[n];
    ctx.textAlign = "right";
    ctx.fillText("inlet →", ix - 12 * dpr, iy + 3 * dpr);
    ctx.textAlign = "left";
    ctx.fillText("→ choke", ox + 12 * dpr, oy + 3 * dpr);
  }

  drawHandles(ctx, dpr) {
    for (let i = 0; i < this.verts.length; i++) {
      const [vx, vz] = this.toScreen(this.verts[i].x, this.verts[i].z);
      const active = i === this.drag || i === this.hover;
      ctx.beginPath();
      ctx.arc(vx, vz, (active ? 6 : 4.5) * dpr, 0, 7);
      ctx.fillStyle = active ? T.ink : T.cream;
      ctx.strokeStyle = T.ink;
      ctx.lineWidth = 1.4 * dpr;
      ctx.fill();
      ctx.stroke();
    }
  }

  drawScale(ctx, cv, dpr) {
    const len = niceStep(this.totalLen, 4);
    const px = len * this.scale;
    const x0 = cv.width - px - 18 * dpr;
    const y = cv.height - 16 * dpr;
    ctx.strokeStyle = T.ink2;
    ctx.lineWidth = 1.2 * dpr;
    ctx.beginPath();
    ctx.moveTo(x0, y - 3 * dpr);
    ctx.lineTo(x0, y + 3 * dpr);
    ctx.moveTo(x0, y);
    ctx.lineTo(x0 + px, y);
    ctx.moveTo(x0 + px, y - 3 * dpr);
    ctx.lineTo(x0 + px, y + 3 * dpr);
    ctx.stroke();
    setFont(ctx, 10.5 * dpr);
    ctx.fillStyle = T.ink2;
    ctx.textAlign = "center";
    ctx.fillText(`${len} m`, x0 + px / 2, y - 6 * dpr);
    ctx.textAlign = "left";
  }

  /// Legend entries actually present in this frame (drawn by app.js).
  regimesPresent(view) {
    const seen = new Set();
    for (let i = 0; i < view.n; i++) seen.add(view.regime[i]);
    return [...seen].sort((a, b) => a - b).map((r) => ({ r, color: REGIME_COLORS[r] }));
  }

  // ---------- editing ----------

  pick(e) {
    const r = this.cv.getBoundingClientRect();
    return [
      ((e.clientX - r.left) * this.cv.width) / r.width,
      ((e.clientY - r.top) * this.cv.height) / r.height,
    ];
  }

  vertexAt(px, pz) {
    for (let i = 0; i < this.verts.length; i++) {
      const [vx, vz] = this.toScreen(this.verts[i].x, this.verts[i].z);
      if (Math.hypot(px - vx, pz - vz) < 14 * devicePixelRatio) return i;
    }
    return -1;
  }

  pointerDown(e) {
    const [px, pz] = this.pick(e);
    const i = this.vertexAt(px, pz);
    if (i < 0) return;
    if (e.altKey && this.verts.length > 2 && i > 0 && i < this.verts.length - 1) {
      this.verts.splice(i, 1);
      this.diams.splice(Math.min(i, this.diams.length - 1), 1);
      this.fit();
      this.onEdit();
      return;
    }
    this.drag = i;
    this.cv.setPointerCapture(e.pointerId);
  }

  pointerMove(e) {
    const [px, pz] = this.pick(e);
    if (this.drag < 0) {
      const h = this.vertexAt(px, pz);
      if (h !== this.hover) this.hover = h;
      this.cv.style.cursor = h >= 0 ? "grab" : "default";
      return;
    }
    const p = this.fromScreen(px, pz);
    const i = this.drag;
    const lo = i > 0 ? this.verts[i - 1].x + 0.5 : -Infinity;
    const hi = i < this.verts.length - 1 ? this.verts[i + 1].x - 0.5 : Infinity;
    if (i > 0) this.verts[i] = { x: Math.min(Math.max(p.x, lo), hi), z: p.z };
    this.cv.style.cursor = "grabbing";
    this.buildScreenPath();
  }

  pointerUp() {
    if (this.drag < 0) return;
    this.drag = -1;
    this.cv.style.cursor = "grab";
    this.fit();
    this.onEdit();
  }

  split(e) {
    const [px, pz] = this.pick(e);
    let best = -1;
    let bd = 28 * devicePixelRatio;
    let bt = 0.5;
    for (let i = 0; i + 1 < this.verts.length; i++) {
      const [ax, az] = this.pts[i];
      const [bx, bz] = this.pts[i + 1];
      const dd = (bx - ax) ** 2 + (bz - az) ** 2 || 1;
      const t = Math.max(0.15, Math.min(0.85, ((px - ax) * (bx - ax) + (pz - az) * (bz - az)) / dd));
      const d = Math.hypot(px - (ax + t * (bx - ax)), pz - (az + t * (bz - az)));
      if (d < bd) {
        bd = d;
        best = i;
        bt = t;
      }
    }
    if (best >= 0) {
      const a = this.verts[best];
      const b = this.verts[best + 1];
      this.verts.splice(best + 1, 0, {
        x: a.x + bt * (b.x - a.x),
        z: a.z + bt * (b.z - a.z),
      });
      this.diams.splice(best, 0, this.diams[best]);
      this.onEdit();
    }
  }

  setDiameter(d) {
    this.diams = this.diams.map(() => d);
  }
}

/// Walk a globally-anchored lattice of features (spacing px, drifting with
/// the flow) and hand back their positions in this cell's local frame.
function forEachOnLattice(sPix, drift, spacing, halfLen, fn) {
  const first = Math.floor((sPix - halfLen - drift) / spacing) - 1;
  const last = Math.ceil((sPix + halfLen - drift) / spacing) + 1;
  for (let k = first; k <= last; k++) fn(k * spacing + drift - sPix, k);
}

function capsule(ctx, x, y, len, h) {
  const r = Math.max(h / 2, 1);
  const half = Math.max(len / 2 - r, 0.5);
  ctx.beginPath();
  ctx.moveTo(x - half, y - r);
  ctx.lineTo(x + half, y - r);
  ctx.arc(x + half, y, r, -Math.PI / 2, Math.PI / 2);
  ctx.lineTo(x - half, y + r);
  ctx.arc(x - half, y, r, Math.PI / 2, -Math.PI / 2);
  ctx.closePath();
  ctx.fill();
}

/// deterministic tiny hash -> [0,1): patterns stay put when scrubbing
function hash(i) {
  let h = (i | 0) * 2654435761;
  h ^= h >> 16;
  h = (h * 2246822519) & 0x7fffffff;
  return (h % 10000) / 10000;
}

function niceStep(span, target) {
  const raw = span / target;
  const p = 10 ** Math.floor(Math.log10(raw));
  const m = raw / p;
  return (m >= 5 ? 5 : m >= 2 ? 2 : 1) * p;
}
