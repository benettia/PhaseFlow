// Pipe view: geometry rendered true to shape, each cell filled by regime.
// Also hosts the polyline editor (drag vertices / split / delete).

const GAS = "#e8c47c", GAS_DEEP = "#d9a441", LIQ = "#1f4e79", LIQ_DEEP = "#16395c";
const INK = "#1c1b18", INK2 = "#6b665c";

export class PipeView {
  constructor(canvas, onEdit) {
    this.cv = canvas;
    this.ctx = canvas.getContext("2d");
    this.onEdit = onEdit; // (vertices) => void, fired after a drag/split/delete
    this.verts = []; // [{x, z}] in metres, derived from scenario segments
    this.diams = []; // per segment
    this.drag = -1;
    canvas.addEventListener("pointerdown", (e) => this.pointerDown(e));
    canvas.addEventListener("pointermove", (e) => this.pointerMove(e));
    canvas.addEventListener("pointerup", () => this.pointerUp());
    canvas.addEventListener("dblclick", (e) => this.split(e));
  }

  setScenario(sc) {
    this.verts = [{ x: 0, z: 0 }];
    this.diams = [];
    let x = 0, z = 0;
    for (const s of sc.segments) {
      const th = (s.angle * Math.PI) / 180;
      x += s.length * Math.cos(th);
      z += s.length * Math.sin(th);
      this.verts.push({ x, z });
      this.diams.push(s.diameter);
    }
    this.fit();
  }

  // vertices -> segments (cells redistributed, ~240 total, min 6 per segment)
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
    const w = this.cv.width, h = this.cv.height;
    let x0 = Infinity, x1 = -Infinity, z0 = Infinity, z1 = -Infinity;
    for (const v of this.verts) {
      x0 = Math.min(x0, v.x); x1 = Math.max(x1, v.x);
      z0 = Math.min(z0, v.z); z1 = Math.max(z1, v.z);
    }
    const pad = 60;
    const sx = (w - 2 * pad) / Math.max(x1 - x0, 1e-6);
    const sz = (h - 2 * pad) / Math.max(z1 - z0, 1e-6);
    this.scale = Math.min(sx, sz, 1e4);
    this.ox = pad - x0 * this.scale + (w - 2 * pad - (x1 - x0) * this.scale) / 2;
    this.oz = h - pad + z0 * this.scale - (h - 2 * pad - (z1 - z0) * this.scale) / 2;
  }

  toScreen(x, z) {
    return [this.ox + x * this.scale, this.oz - z * this.scale];
  }
  fromScreen(px, pz) {
    return { x: (px - this.ox) / this.scale, z: (this.oz - pz) / this.scale };
  }

  pick(e) {
    const r = this.cv.getBoundingClientRect();
    const px = ((e.clientX - r.left) * this.cv.width) / r.width;
    const pz = ((e.clientY - r.top) * this.cv.height) / r.height;
    return [px, pz];
  }

  pointerDown(e) {
    const [px, pz] = this.pick(e);
    for (let i = 0; i < this.verts.length; i++) {
      const [vx, vz] = this.toScreen(this.verts[i].x, this.verts[i].z);
      if (Math.hypot(px - vx, pz - vz) < 12) {
        if (e.altKey && this.verts.length > 2 && i > 0 && i < this.verts.length - 1) {
          this.verts.splice(i, 1);
          this.diams.splice(Math.min(i, this.diams.length - 1) - 0, 1);
          this.fit();
          this.onEdit();
          return;
        }
        this.drag = i;
        this.cv.setPointerCapture(e.pointerId);
        return;
      }
    }
  }

  pointerMove(e) {
    if (this.drag < 0) return;
    const [px, pz] = this.pick(e);
    const p = this.fromScreen(px, pz);
    const i = this.drag;
    // keep x monotone so segments never fold back
    const lo = i > 0 ? this.verts[i - 1].x + 0.5 : -Infinity;
    const hi = i < this.verts.length - 1 ? this.verts[i + 1].x - 0.5 : Infinity;
    if (i > 0) this.verts[i] = { x: Math.min(Math.max(p.x, lo), hi), z: p.z };
    this.dirty = true;
  }

  pointerUp() {
    if (this.drag < 0) return;
    this.drag = -1;
    this.fit();
    this.onEdit();
  }

  split(e) {
    const [px, pz] = this.pick(e);
    // nearest segment midpointish insertion
    let best = -1, bd = 25;
    for (let i = 0; i + 1 < this.verts.length; i++) {
      const [ax, az] = this.toScreen(this.verts[i].x, this.verts[i].z);
      const [bx, bz] = this.toScreen(this.verts[i + 1].x, this.verts[i + 1].z);
      const t = Math.max(0.1, Math.min(0.9,
        ((px - ax) * (bx - ax) + (pz - az) * (bz - az)) / ((bx - ax) ** 2 + (bz - az) ** 2 || 1)));
      const d = Math.hypot(px - (ax + t * (bx - ax)), pz - (az + t * (bz - az)));
      if (d < bd) { bd = d; best = i; this.bestT = t; }
    }
    if (best >= 0) {
      const a = this.verts[best], b = this.verts[best + 1];
      this.verts.splice(best + 1, 0, { x: a.x + this.bestT * (b.x - a.x), z: a.z + this.bestT * (b.z - a.z) });
      this.diams.splice(best, 0, this.diams[best]);
      this.onEdit();
    }
  }

  setDiameter(d) {
    this.diams = this.diams.map(() => d);
  }

  // --- drawing ---
  draw(sim, simTime) {
    const { ctx, cv } = this;
    ctx.clearRect(0, 0, cv.width, cv.height);
    if (!sim) return;
    const n = sim.n_cells();
    const alpha = sim.alpha(), regime = sim.regime(), vg = sim.vg(), vl = sim.vl();
    const x = sim.x_mid(), elev = sim.elev(), diam = sim.diam();
    // reconstruct planar coordinates: walk the polyline by arc length
    const sxy = this.cellScreens(x, elev, n);
    const dmax = Math.max(...this.diams);
    for (let i = 0; i < n; i++) {
      const [cx, cz, ang, ds] = sxy[i];
      const wpx = Math.min(34, Math.max(9, 16 * Math.sqrt(diam[i] / dmax)));
      ctx.save();
      ctx.translate(cx, cz);
      ctx.rotate(ang);
      this.drawCell(ctx, ds + 1.2, wpx, alpha[i], regime[i], vg[i], vl[i], simTime, i, ang);
      ctx.restore();
    }
    // ink outline along the pipe
    ctx.strokeStyle = INK;
    ctx.lineWidth = 1.2;
    for (const off of [-1, 1]) {
      ctx.beginPath();
      for (let i = 0; i < n; i++) {
        const [cx, cz, ang] = sxy[i];
        const wpx = Math.min(34, Math.max(9, 16 * Math.sqrt(diam[i] / dmax))) / 2 + 0.6;
        const px = cx - Math.sin(ang) * -off * wpx, pz = cz + Math.cos(ang) * -off * wpx;
        i ? ctx.lineTo(px, pz) : ctx.moveTo(px, pz);
      }
      ctx.stroke();
    }
    // vertex handles
    for (let i = 0; i < this.verts.length; i++) {
      const [vx, vz] = this.toScreen(this.verts[i].x, this.verts[i].z);
      ctx.beginPath();
      ctx.arc(vx, vz, 4.5, 0, 7);
      ctx.fillStyle = "#f4efe4";
      ctx.strokeStyle = INK;
      ctx.lineWidth = 1.4;
      ctx.fill();
      ctx.stroke();
    }
    // elevation scale note
    ctx.fillStyle = INK2;
    ctx.font = "11px ui-monospace, monospace";
    const zs = this.verts.map((v) => v.z);
    ctx.fillText(`Δz ${(Math.max(...zs) - Math.min(...zs)).toFixed(1)} m · L ${this.verts.length > 1 ? this.segments().reduce((a, s) => a + s.length, 0).toFixed(0) : 0} m`, 12, 16);
  }

  cellScreens(x, elev, n) {
    // map arc-length positions onto the drawn polyline
    const segs = this.segments();
    const out = [];
    let acc = 0, si = 0;
    for (let i = 0; i < n; i++) {
      while (si < segs.length - 1 && x[i] > acc + segs[si].length) acc += segs[si++].length;
      const t = Math.min(1, Math.max(0, (x[i] - acc) / segs[si].length));
      const a = this.verts[si], b = this.verts[si + 1];
      const [ax, az] = this.toScreen(a.x, a.z);
      const [bx, bz] = this.toScreen(b.x, b.z);
      const ang = Math.atan2(bz - az, bx - ax);
      const dsPer = Math.hypot(bx - ax, bz - az) / (n * segs[si].length / segs.reduce((s, g) => s + g.length, 0));
      out.push([ax + t * (bx - ax), az + t * (bz - az), ang, Math.hypot(bx - ax, bz - az) * (segs[si].length / (segs[si].cells * segs[si].length)) * 1 || dsPer]);
    }
    // cell screen length: distance to neighbour
    for (let i = 0; i < n; i++) {
      const j = Math.min(i + 1, n - 1), k = Math.max(i - 1, 0);
      out[i][3] = Math.max(2, Math.hypot(out[j][0] - out[k][0], out[j][1] - out[k][1]) / Math.max(1, j - k));
    }
    return out;
  }

  // local frame: x along pipe (cell centred at 0), y transverse; length ds, width w
  drawCell(ctx, ds, w, a, reg, vg, vl, t, idx, ang) {
    const h = w / 2, l = ds / 2 + 0.6;
    ctx.beginPath();
    ctx.rect(-l, -h, 2 * l, w);
    ctx.clip();
    const liq = 1 - a;
    switch (reg) {
      case 7: ctx.fillStyle = LIQ; ctx.fillRect(-l, -h, 2 * l, w); break;
      case 8: ctx.fillStyle = GAS; ctx.fillRect(-l, -h, 2 * l, w); break;
      case 0: case 1: { // stratified: actual liquid level at gravity-bottom
        ctx.fillStyle = GAS;
        ctx.fillRect(-l, -h, 2 * l, w);
        ctx.fillStyle = LIQ;
        // gravity-down in local frame: screen +y rotated by -ang
        const down = Math.cos(ang) >= 0 ? 1 : -1;
        const lvl = h - liq * w; // interface y (local, down = +y when down=1)
        if (down > 0) ctx.fillRect(-l, lvl, 2 * l, liq * w);
        else ctx.fillRect(-l, -h, 2 * l, liq * w);
        if (reg === 1) { // wavy interface
          ctx.strokeStyle = LIQ_DEEP;
          ctx.lineWidth = 1;
          ctx.beginPath();
          const y0 = down > 0 ? lvl : -h + liq * w;
          for (let px = -l; px <= l; px += 3) {
            const yy = y0 + 1.5 * Math.sin(0.6 * px + idx + 8 * t);
            px === -l ? ctx.moveTo(px, yy) : ctx.lineTo(px, yy);
          }
          ctx.stroke();
        }
        break;
      }
      case 2: { // slug: Taylor bubble / liquid slug alternation, advected
        ctx.fillStyle = LIQ;
        ctx.fillRect(-l, -h, 2 * l, w);
        ctx.fillStyle = GAS;
        const lambda = 6 * w, phase = ((idx * 0.37 + (vg * t) / (lambda / 18)) % 1 + 1) % 1;
        const off = phase * lambda - lambda;
        for (let px = off - lambda; px < l + lambda; px += lambda) {
          const bl = lambda * Math.min(0.75, 0.3 + 0.6 * a);
          roundedBlob(ctx, px, 0, bl, Math.max(2, w * 0.62));
        }
        break;
      }
      case 3: { // annular: gas core, liquid film
        ctx.fillStyle = GAS;
        ctx.fillRect(-l, -h, 2 * l, w);
        ctx.fillStyle = LIQ;
        const film = Math.max(1, (liq * w) / 2);
        ctx.fillRect(-l, -h, 2 * l, film);
        ctx.fillRect(-l, h - film, 2 * l, film);
        break;
      }
      case 6: { // churn: broken alternation
        ctx.fillStyle = LIQ;
        ctx.fillRect(-l, -h, 2 * l, w);
        ctx.fillStyle = GAS_DEEP;
        for (let k = 0; k < 4; k++) {
          const px = -l + ((hash(idx * 7 + k) + 3.0 * t * Math.abs(vg)) % (2 * l));
          const py = (hash(idx * 13 + k) - 0.5) * w * 0.7;
          roundedBlob(ctx, px, py, w * (0.4 + 0.5 * hash(idx + k * 31)), w * 0.3);
        }
        break;
      }
      default: { // bubbly / dispersed: stipple density ~ alpha
        ctx.fillStyle = LIQ;
        ctx.fillRect(-l, -h, 2 * l, w);
        ctx.fillStyle = GAS;
        const nb = Math.round(2 + a * 26);
        for (let k = 0; k < nb; k++) {
          const px = -l + ((hash(idx * 3 + k * 17) * 2 * l + 2.2 * t * vg * 4) % (2 * l) + 2 * l) % (2 * l);
          const py = (hash(idx * 11 + k * 5) - 0.5) * (w - 4);
          ctx.beginPath();
          ctx.arc(px - l * 0, py, 1.4, 0, 7);
          ctx.fill();
        }
      }
    }
  }
}

function roundedBlob(ctx, x, y, len, w) {
  ctx.beginPath();
  ctx.ellipse(x, y, Math.max(len / 2, 1), Math.max(w / 2, 1), 0, 0, 7);
  ctx.fill();
}

// deterministic tiny hash -> [0,1)
function hash(i) {
  let h = (i | 0) * 2654435761;
  h ^= h >> 16;
  h = (h * 2246822519) & 0x7fffffff;
  return (h % 10000) / 10000;
}
