// Rolling record of the run: enough to redraw any past instant, and enough
// to put the solver back there exactly.
//
// Each frame keeps the conserved state (f64, for exact rollback via
// WasmSim.load_state) plus display fields in f32 (rendering precision) and
// the design report for that instant — so scrubbing back shows the numbers
// that were true then, not the numbers that are true now.
// Memory is budgeted, not fixed: ~24 MB of frames whatever the cell count.

const BUDGET_BYTES = 24e6;

export class History {
  constructor(n) {
    this.n = n;
    // conserved state f64 (5 fields) + 5 display f32 + regime u8 + report
    const perFrame = n * (5 * 8 + 5 * 4 + 1) + 200;
    this.cap = Math.max(120, Math.min(1200, Math.floor(BUDGET_BYTES / perFrame)));
    this.frames = [];
    this.dropped = 0; // frames evicted from the front (for absolute indexing)
  }

  get length() {
    return this.frames.length;
  }

  clear() {
    this.frames = [];
    this.dropped = 0;
  }

  /// Capture the live solver. `state` is the f64 snapshot; display arrays are
  /// copied out of wasm memory (the views are invalidated on memory growth).
  capture(sim) {
    const f = {
      t_sim: sim.time(),
      steps: sim.steps(),
      dt: sim.dt_last(),
      state: sim.save_state().slice(),
      alpha: Float32Array.from(sim.alpha()),
      p: Float32Array.from(sim.p()),
      t: Float32Array.from(sim.temperature()),
      vg: Float32Array.from(sim.vg()),
      vl: Float32Array.from(sim.vl()),
      regime: Uint8Array.from(sim.regime()),
      report: sim.report(),
    };
    this.frames.push(f);
    while (this.frames.length > this.cap) {
      this.frames.shift();
      this.dropped++;
    }
    return f;
  }

  at(i) {
    return this.frames[Math.max(0, Math.min(this.frames.length - 1, i))];
  }

  last() {
    return this.frames[this.frames.length - 1];
  }

  /// Drop everything after `i` — the future stops existing once you resume
  /// from the past.
  truncateAfter(i) {
    this.frames.length = Math.min(this.frames.length, i + 1);
  }

  /// Frame index nearest a given sim time (binary search; times increase).
  indexOfTime(t) {
    let lo = 0;
    let hi = this.frames.length - 1;
    while (lo < hi) {
      const mid = (lo + hi) >> 1;
      if (this.frames[mid].t_sim < t) lo = mid + 1;
      else hi = mid;
    }
    return lo;
  }

  /// Series of one probe cell over the whole record: {t, v} typed arrays.
  series(cell, field) {
    const m = this.frames.length;
    const t = new Float64Array(m);
    const v = new Float64Array(m);
    for (let i = 0; i < m; i++) {
      t[i] = this.frames[i].t_sim;
      v[i] = this.frames[i][field][cell];
    }
    return { t, v };
  }
}
