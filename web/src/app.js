// phase-flow panel: boots the wasm solver, owns the run loop, the timeline,
// and the controls.
//
// Two modes. LIVE steps the solver and records frames. REVIEW renders a past
// frame and leaves the solver untouched — until you press run, which rolls
// the solver back to that frame (exactly: see Sim::load_state) and continues
// from there, so you can rewind, change the choke, and replay the instant.

import init, { WasmSim, classify_point } from "../pkg/phase_flow_wasm.js";
import { StripChart } from "./charts.js";
import { History } from "./history.js";
import { decodeHash, encodeHash, PRESETS } from "./presets.js";
import { PipeView } from "./render.js";
import { TDMap } from "./tdmap.js";
import { REGIME_COLORS, REGIME_SHORT } from "./theme.js";

const $ = (id) => document.getElementById(id);
const clamp = (v, a, b) => Math.max(a, Math.min(b, v));

let sim = null;
let scenario = null;
let tape = null; // rolling record of the run
let tdmap = null;
let running = true;
let reviewing = false;
let playhead = 0;
let speed = 1;
let budget = 1; // solver share of the frame, throttled if we fall behind
let probes = [0, 0, 0];
let slamT = -1;
let lastCapture = 0;
let lastT = 0;
let legendMask = -1;

const CAPTURE_MS = 40;

const pipe = new PipeView($("pipe"), onGeometryEdit);
const chartP = new StripChart($("chart-p"), {
  title: "pressure at probes",
  unit: " kPa",
  scale: 1e-3,
  field: "p",
  decimals: 1,
});
const chartH = new StripChart($("chart-h"), {
  title: "liquid holdup at probes",
  field: "alpha",
  map: (a) => 1 - a,
  decimals: 3,
});

// ---------- views: one shape, whether it comes from the solver or the tape ----------

function liveView() {
  return {
    n: sim.n_cells(),
    alpha: sim.alpha(),
    p: sim.p(),
    vg: sim.vg(),
    vl: sim.vl(),
    regime: sim.regime(),
    t: sim.time(),
    dt: sim.dt_last(),
    probes,
  };
}

function frameView(f) {
  return {
    n: f.alpha.length,
    alpha: f.alpha,
    p: f.p,
    vg: f.vg,
    vl: f.vl,
    regime: f.regime,
    t: f.t,
    dt: f.dt,
    probes,
  };
}

function currentView() {
  if (reviewing && tape?.length) return frameView(tape.at(playhead));
  return sim ? liveView() : null;
}

// ---------- scenario ----------

function loadScenario(sc, { updateHash = true } = {}) {
  scenario = structuredClone(sc);
  try {
    sim = new WasmSim(JSON.stringify(scenario));
    banner("");
  } catch (e) {
    banner(String(e));
    sim = null;
    return;
  }
  const n = sim.n_cells();
  tape = new History(n);
  probes = [Math.floor(0.1 * n), sim.min_elev_cell(), Math.floor(0.95 * n)];
  pipe.setScenario(scenario);
  pipe.setProbes(probes);
  setReviewing(false);
  playhead = 0;
  capture(performance.now());
  syncControls();
  if (updateHash) pushHash();
  drawAll();
}

function pushHash() {
  window.history.replaceState(null, "", "#" + encodeHash(scenario));
}

function onGeometryEdit() {
  if (!scenario) return;
  scenario.segments = pipe.segments();
  loadScenario(scenario);
}

// ---------- tape / timeline ----------

function capture(now) {
  if (!sim || !tape) return;
  tape.capture(sim);
  lastCapture = now;
  if (!reviewing) playhead = tape.length - 1;
  syncScrub();
}

function setReviewing(on) {
  reviewing = on;
  document.body.classList.toggle("reviewing", on);
  syncRunButton();
}

function scrubTo(i) {
  if (!tape?.length) return;
  playhead = clamp(i, 0, tape.length - 1);
  const atLive = playhead === tape.length - 1;
  if (!atLive) {
    setReviewing(true);
    running = false;
  } else {
    setReviewing(false);
  }
  syncRunButton();
  syncScrub();
  drawAll();
}

/// Roll the solver back to the frame on screen. The future is dropped —
/// what you replay from here is a new branch.
function resumeHere() {
  if (!reviewing || !tape?.length) return;
  const f = tape.at(playhead);
  try {
    sim.load_state(f.state, f.t, f.steps);
  } catch (e) {
    banner(String(e));
    return;
  }
  tape.truncateAfter(playhead);
  setReviewing(false);
  playhead = tape.length - 1;
  syncScrub();
}

function syncScrub() {
  const m = tape?.length ?? 0;
  const el = $("scrub");
  el.max = String(Math.max(0, m - 1));
  el.value = String(playhead);
  if (m) {
    $("t-first").textContent = `${tape.at(0).t.toFixed(1)} s`;
    $("t-last").textContent = `${tape.last().t.toFixed(1)} s`;
    const span = tape.last().t - tape.at(0).t;
    $("frame-info").textContent =
      `frame ${playhead + 1}/${m} · ${span.toFixed(1)} s recorded`;
  }
}

function syncRunButton() {
  const b = $("run");
  b.textContent = reviewing ? "▶" : running ? "❚❚" : "▶";
  b.title = reviewing
    ? "roll the solver back to this frame and continue"
    : running
      ? "pause  (space)"
      : "run  (space)";
}

// ---------- controls ----------

const wgMap = (v) => (v <= 0 ? 0 : 10 ** (-4 + v * 3.3));
const wlMap = (v) => (v <= 0 ? 0 : 10 ** (-2 + v * 4));
const wgInv = (w) => (w <= 0 ? 0 : clamp((Math.log10(w) + 4) / 3.3, 0, 1));
const wlInv = (w) => (w <= 0 ? 0 : clamp((Math.log10(w) + 2) / 4, 0, 1));

function bind(id, fmt, apply) {
  const el = $(id);
  const out = $(id + "-out");
  el.addEventListener("input", () => {
    out.textContent = fmt(parseFloat(el.value));
    apply(parseFloat(el.value));
  });
  return { el, out };
}

bind("wg", (v) => fmtRate(wgMap(v)), (v) => setBc({ wg: wgMap(v) }));
bind("wl", (v) => fmtRate(wlMap(v)), (v) => setBc({ wl: wlMap(v) }));
bind("pout", (v) => `${v.toFixed(1)} bar`, (v) => setBc({ p: v * 1e5 }));
bind("choke", (v) => `${Math.round(v * 100)} %`, (v) => setBc({ choke: v }));
bind("diam", (v) => `${v.toFixed(3)} m`, (v) => {
  pipe.setDiameter(v);
  onGeometryEdit();
});
bind("speed", (v) => fmtSpeed(2 ** v), (v) => {
  speed = 2 ** v;
  $("spd").textContent = fmtSpeed(speed);
});

function fmtRate(w) {
  if (w === 0) return "0";
  if (w >= 100) return `${w.toFixed(0)} kg/s`;
  if (w >= 1) return `${w.toFixed(2)} kg/s`;
  return `${w.toPrecision(2)} kg/s`;
}

function fmtSpeed(s) {
  return s >= 1 ? `${s.toFixed(0)}×` : `${s.toFixed(2)}×`;
}

function setBc(part) {
  if (!scenario || !sim) return;
  if ("wg" in part) scenario.inlet.wg = part.wg;
  if ("wl" in part) scenario.inlet.wl = part.wl;
  if ("p" in part) scenario.outlet.p = part.p;
  if ("choke" in part) scenario.outlet.choke = part.choke;
  sim.set_bc(scenario.inlet.wg, scenario.inlet.wl, scenario.outlet.p, scenario.outlet.choke);
  pushHash();
}

function syncControls() {
  const set = (id, value, text) => {
    $(id).value = String(value);
    $(id + "-out").textContent = text;
  };
  set("wg", wgInv(scenario.inlet.wg), fmtRate(scenario.inlet.wg));
  set("wl", wlInv(scenario.inlet.wl), fmtRate(scenario.inlet.wl));
  set("pout", scenario.outlet.p / 1e5, `${(scenario.outlet.p / 1e5).toFixed(1)} bar`);
  set("choke", scenario.outlet.choke, `${Math.round(scenario.outlet.choke * 100)} %`);
  set("diam", scenario.segments[0].diameter, `${scenario.segments[0].diameter.toFixed(3)} m`);
  set("speed", Math.log2(speed), fmtSpeed(speed));
  $("muscl").checked = scenario.options?.muscl !== false;
  $("feedback").checked = !!scenario.options?.regime_feedback;
  syncRunButton();
}

$("muscl").addEventListener("change", () => {
  scenario.options = scenario.options || {};
  scenario.options.muscl = $("muscl").checked;
  sim?.set_muscl($("muscl").checked);
  pushHash();
});

$("feedback").addEventListener("change", () => {
  scenario.options = scenario.options || {};
  scenario.options.regime_feedback = $("feedback").checked;
  loadScenario(scenario); // a physics change is deliberate: restart clean
});

$("run").addEventListener("click", () => {
  if (reviewing) {
    resumeHere();
    running = true;
  } else {
    running = !running;
  }
  syncRunButton();
});

$("stepbtn").addEventListener("click", () => {
  if (!sim) return;
  if (reviewing) resumeHere();
  running = false;
  try {
    sim.single_step();
  } catch (e) {
    banner(String(e));
    return;
  }
  capture(performance.now());
  syncRunButton();
  drawAll();
});

$("scrub").addEventListener("input", () => scrubTo(parseInt($("scrub").value, 10)));
$("to-start").addEventListener("click", () => scrubTo(0));
$("back").addEventListener("click", () => scrubTo(playhead - 1));
$("fwd").addEventListener("click", () => scrubTo(playhead + 1));
$("to-live").addEventListener("click", () => scrubTo((tape?.length ?? 1) - 1));
$("slam").addEventListener("click", () => {
  if (!sim) return;
  if (reviewing) resumeHere();
  running = true;
  slamT = sim.time();
  syncRunButton();
});

addEventListener("keydown", (e) => {
  if (e.target.matches("input, textarea")) return;
  const k = e.key;
  if (k === " ") {
    e.preventDefault();
    $("run").click();
  } else if (k === "ArrowLeft") {
    e.preventDefault();
    scrubTo(playhead - (e.shiftKey ? 10 : 1));
  } else if (k === "ArrowRight") {
    e.preventDefault();
    scrubTo(playhead + (e.shiftKey ? 10 : 1));
  } else if (k === "Home") {
    scrubTo(0);
  } else if (k === "End") {
    scrubTo((tape?.length ?? 1) - 1);
  }
});

// ---------- presets & legend ----------

for (const name of Object.keys(PRESETS)) {
  const b = document.createElement("button");
  b.className = "preset";
  b.textContent = name;
  b.addEventListener("click", () => {
    document.querySelectorAll(".preset").forEach((x) => x.classList.remove("on"));
    b.classList.add("on");
    running = true;
    slamT = -1;
    loadScenario(PRESETS[name]);
    syncRunButton();
  });
  $("presets").append(b);
}

REGIME_SHORT.forEach((name, i) => {
  const li = document.createElement("li");
  li.innerHTML = `<i style="background:${REGIME_COLORS[i]}"></i>${name}`;
  $("legend-list").append(li);
});

function updateLegend(view) {
  let mask = 0;
  for (let i = 0; i < view.n; i++) mask |= 1 << view.regime[i];
  if (mask === legendMask) return;
  legendMask = mask;
  [...$("legend-list").children].forEach((li, i) => {
    li.classList.toggle("on", (mask >> i) & 1);
  });
}

// ---------- loop ----------

function frame(now) {
  requestAnimationFrame(frame);
  const wallDt = Math.min(0.05, (now - lastT) / 1000 || 0.016);
  lastT = now;

  if (sim && running && !reviewing) {
    if (slamT >= 0) {
      const f = Math.max(0, 1 - (sim.time() - slamT) / 0.1);
      scenario.outlet.choke = f;
      sim.set_bc(scenario.inlet.wg, scenario.inlet.wl, scenario.outlet.p, f);
      $("choke").value = String(f);
      $("choke-out").textContent = `${Math.round(f * 100)} %`;
      if (f <= 0) slamT = -1;
    }
    const t0 = performance.now();
    try {
      sim.step(wallDt * 1000 * speed * budget);
    } catch (e) {
      banner(String(e));
      running = false;
      syncRunButton();
    }
    const cost = performance.now() - t0;
    budget = cost > 22 ? Math.max(0.04, budget * 0.7) : Math.min(1, budget * 1.06);
    if (now - lastCapture >= CAPTURE_MS) capture(now);
  }
  drawAll();
}

function drawAll() {
  const view = currentView();
  if (!view) return;
  pipe.draw(view);
  chartP.draw(tape, probes, playhead);
  chartH.draw(tape, probes, playhead);
  tdmap.draw(view, scenario.segments[0].diameter);
  updateLegend(view);
  $("t-now").textContent = view.t.toFixed(3);
  $("dt").textContent = view.dt ? `${view.dt.toExponential(1)} s` : "—";
  $("lag").textContent = budget < 0.99 && running && !reviewing ? `lagging ×${budget.toFixed(2)}` : "";
}

function banner(msg) {
  const b = $("banner");
  b.style.display = msg ? "block" : "none";
  b.textContent = msg ? String(msg).replace(/^Error:\s*/, "") : "";
}

function sizeCanvases() {
  for (const cv of document.querySelectorAll("canvas")) {
    const r = cv.getBoundingClientRect();
    cv.width = Math.max(50, (r.width * devicePixelRatio) | 0);
    cv.height = Math.max(50, (r.height * devicePixelRatio) | 0);
  }
  pipe.fit();
}
addEventListener("resize", () => {
  sizeCanvases();
  drawAll();
});

// ---------- boot ----------

init().then(() => {
  tdmap = new TDMap($("tdmap"), classify_point);
  sizeCanvases();
  const fromHash = location.hash.length > 1 ? decodeHash(location.hash.slice(1)) : null;
  const names = Object.keys(PRESETS);
  if (fromHash) {
    loadScenario(fromHash, { updateHash: false });
  } else {
    document.querySelectorAll(".preset")[names.indexOf("severe slugging")]?.classList.add("on");
    loadScenario(PRESETS["severe slugging"]);
  }
  running = true;
  syncRunButton();
  window.PHASEFLOW_READY = true;
  requestAnimationFrame(frame);
});
