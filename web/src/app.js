// phase-flow panel: boots the wasm solver, owns the run loop, the timeline,
// the controls and the design report.
//
// Two modes. LIVE steps the solver and records frames. REVIEW renders a past
// frame and leaves the solver untouched — until you press run, which rolls
// the solver back to that frame (exactly: see Sim::load_state) and continues
// from there, so you can rewind, change the choke, and replay the instant.

import init, { WasmSim } from "../pkg/phase_flow_wasm.js";
import { StripChart } from "./charts.js";
import { History } from "./history.js";
import { ProfileView } from "./profile.js";
import { decodeHash, encodeHash, FLUIDS, PRESETS } from "./presets.js";
import { Readout } from "./readout.js";
import { PipeView } from "./render.js";
import { TDMap } from "./tdmap.js";
import { REGIME_COLORS, REGIME_SHORT } from "./theme.js";

const $ = (id) => document.getElementById(id);
const clamp = (v, a, b) => Math.max(a, Math.min(b, v));
const K0 = 273.15;

let sim = null;
let scenario = null;
let tape = null; // rolling record of the run
let geom = null; // cell centres + segment breaks, for the profile view
let running = true;
let reviewing = false;
let playhead = 0;
let speed = 1;
let budget = 1; // solver share of the frame, throttled if we fall behind
let probes = [0, 0, 0];
let slamT = -1;
let settleUntil = -1;
let settleRef = null;
let lastCapture = 0;
let lastT = 0;
let legendMask = -1;
let activeView = "profiles";

const CAPTURE_MS = 40;

const pipe = new PipeView($("pipe"), onGeometryEdit);
const profile = new ProfileView($("profile"));
const readout = new Readout($("report"));
let tdmap = null;
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
profile.redraw = () => drawAll();

// ---------- views: one shape, whether it comes from the solver or the tape ----------

function liveView() {
  return {
    n: sim.n_cells(),
    alpha: sim.alpha(),
    p: sim.p(),
    t: sim.temperature(),
    vg: sim.vg(),
    vl: sim.vl(),
    regime: sim.regime(),
    time: sim.time(),
    dt: sim.dt_last(),
    report: sim.report(),
    xs: geom.x,
    probes,
  };
}

function frameView(f) {
  return {
    n: f.alpha.length,
    alpha: f.alpha,
    p: f.p,
    t: f.t,
    vg: f.vg,
    vl: f.vl,
    regime: f.regime,
    time: f.t_sim,
    dt: f.dt,
    report: f.report,
    xs: geom.x,
    probes,
  };
}

function currentView() {
  if (!sim || !geom) return null;
  if (reviewing && tape?.length) return frameView(tape.at(playhead));
  return liveView();
}

// ---------- scenario ----------

function loadScenario(sc, { updateHash = true } = {}) {
  scenario = structuredClone(sc);
  scenario.options = scenario.options || {};
  scenario.fluid = scenario.fluid || { preset: "air-water" };
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
  geom = buildGeometry();
  probes = pickProbes(n);
  // a flat line has no elevation story to tell, so give its vertical space
  // to the profiles instead of to empty background (flex-grow, not just the
  // basis: both children grow, so a basis alone barely moves the split)
  const relief = geom.relief > 0.02 * geom.total;
  $("pipe-wrap").style.flex = relief ? "5 1 44%" : "2 1 20%";
  $("analysis").style.flex = relief ? "5 1 44%" : "8 1 62%";
  pipe.setScenario(scenario);
  pipe.setProbes(probes);
  profile.setPanels(
    sim.thermal()
      ? ["pressure", "holdup", "velocity", "temperature"]
      : ["pressure", "holdup", "velocity"],
  );
  setReviewing(false);
  playhead = 0;
  slamT = -1;
  settleUntil = -1;
  capture(performance.now());
  syncControls();
  if (updateHash) pushHash();
  drawAll();
}

/// Cell centres and segment boundaries in metres — the profile view's x axis.
function buildGeometry() {
  const xs = Array.from(sim.x_mid());
  const elev = Array.from(sim.elev());
  const breaks = [];
  let s = 0;
  for (const seg of scenario.segments) {
    s += seg.length;
    breaks.push(s);
  }
  return {
    x: xs,
    breaks: breaks.slice(0, -1),
    total: s,
    relief: Math.max(...elev) - Math.min(...elev),
  };
}

/// Probe cells: inlet region, the low point (where liquid collects), and the
/// outlet region. On a flat line there is no low point, so take mid-length.
function pickProbes(n) {
  const low = geom.relief > 1e-6 ? sim.min_elev_cell() : Math.floor(0.5 * n);
  return [Math.floor(0.1 * n), low, Math.floor(0.95 * n)];
}

function pushHash() {
  window.history.replaceState(null, "", "#" + encodeHash(scenario));
}

function onGeometryEdit() {
  if (!scenario) return;
  scenario.segments = pipe.segments();
  loadScenario(scenario);
}

/// Restart with an edited scenario, keeping the panel state sensible.
function rebuild() {
  running = true;
  loadScenario(scenario);
  syncRunButton();
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
  $("report-when").textContent = on ? "at playhead" : "";
  syncRunButton();
}

function scrubTo(i) {
  if (!tape?.length) return;
  playhead = clamp(i, 0, tape.length - 1);
  const atLive = playhead === tape.length - 1;
  setReviewing(!atLive);
  if (!atLive) running = false;
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
    sim.load_state(f.state, f.t_sim, f.steps);
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
    $("t-first").textContent = `${tape.at(0).t_sim.toFixed(1)} s`;
    $("t-last").textContent = `${tape.last().t_sim.toFixed(1)} s`;
    const span = tape.last().t_sim - tape.at(0).t_sim;
    $("frame-info").textContent = `frame ${playhead + 1}/${m} · ${span.toFixed(1)} s recorded`;
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

const logMap = (lo, span) => (v) => (v <= 0 ? 0 : 10 ** (lo + v * span));
const logInv = (lo, span) => (w) => (w <= 0 ? 0 : clamp((Math.log10(w) - lo) / span, 0, 1));
const wgMap = logMap(-4, 4.3);
const wlMap = logMap(-2, 4);
const wgInv = logInv(-4, 4.3);
const wlInv = logInv(-2, 4);

function bind(id, fmt, apply) {
  const el = $(id);
  const out = $(id + "-out");
  el.addEventListener("input", () => {
    const v = parseFloat(el.value);
    out.textContent = fmt(v);
    apply(v);
  });
  return el;
}

bind("wg", (v) => fmtRate(wgMap(v)), (v) => setBc({ wg: wgMap(v) }));
bind("wl", (v) => fmtRate(wlMap(v)), (v) => setBc({ wl: wlMap(v) }));
bind("pout", (v) => fmtBar(10 ** v), (v) => setBc({ p: 10 ** v * 1e5 }));
bind("choke", (v) => `${Math.round(v * 100)} %`, (v) => setBc({ choke: v }));
bind("speed", (v) => fmtSpeed(2 ** v), (v) => {
  speed = 2 ** v;
  $("spd").textContent = fmtSpeed(speed);
});
// geometry and property changes rebuild the sim: they are not boundary
// conditions, they are a different pipe
bindCommit("diam", (v) => `${v.toFixed(3)} m`, (v) => {
  pipe.setDiameter(v);
  scenario.segments = pipe.segments();
  rebuild();
});
bindCommit("rough", (v) => fmtRough(10 ** v), (v) => {
  scenario.options.roughness = 10 ** v;
  for (const s of scenario.segments) delete s.roughness;
  rebuild();
});
bind("tin", (v) => fmtC(v), (v) => setTemps({ tin: v }));
bind("tamb", (v) => fmtC(v), (v) => setTemps({ tamb: v }));
bindCommit("uwall", (v) => `${(10 ** v - 1).toFixed(1)} W/m²K`, (v) => {
  scenario.options.u_wall = 10 ** v - 1;
  for (const s of scenario.segments) delete s.u_wall;
  rebuild();
});

/// Slider that previews while dragging but only rebuilds the sim on release —
/// rebuilding 60×/s while dragging would throw away the run every frame.
function bindCommit(id, fmt, apply) {
  const el = $(id);
  const out = $(id + "-out");
  el.addEventListener("input", () => {
    out.textContent = fmt(parseFloat(el.value));
  });
  for (const ev of ["change", "pointerup", "keyup"]) {
    el.addEventListener(ev, () => apply(parseFloat(el.value)));
  }
}

function fmtRate(w) {
  if (w === 0) return "0";
  if (w >= 100) return `${w.toFixed(0)} kg/s`;
  if (w >= 1) return `${w.toFixed(2)} kg/s`;
  return `${w.toPrecision(2)} kg/s`;
}
function fmtBar(b) {
  return b >= 10 ? `${b.toFixed(0)} bar` : b >= 1 ? `${b.toFixed(2)} bar` : `${(b * 1e3).toFixed(0)} mbar`;
}
function fmtSpeed(s) {
  return s >= 1 ? `${s.toFixed(0)}×` : `${s.toFixed(2)}×`;
}
function fmtC(k) {
  return `${(k - K0).toFixed(0)} °C`;
}
function fmtRough(m) {
  return m >= 1e-3 ? `${(m * 1e3).toFixed(2)} mm` : `${(m * 1e6).toFixed(0)} µm`;
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

function setTemps(part) {
  if (!scenario || !sim) return;
  if ("tin" in part) scenario.inlet.t = part.tin;
  if ("tamb" in part) scenario.options.t_ambient = part.tamb;
  sim.set_temperatures(scenario.inlet.t ?? fluidRef(), scenario.options.t_ambient ?? fluidRef());
  pushHash();
}

function fluidRef() {
  return sim ? sim.fluid().tRef : 288.15;
}

function syncControls() {
  const set = (id, value, text) => {
    $(id).value = String(value);
    $(id + "-out").textContent = text;
  };
  set("wg", wgInv(scenario.inlet.wg), fmtRate(scenario.inlet.wg));
  set("wl", wlInv(scenario.inlet.wl), fmtRate(scenario.inlet.wl));
  set("pout", Math.log10(scenario.outlet.p / 1e5), fmtBar(scenario.outlet.p / 1e5));
  set("choke", scenario.outlet.choke, `${Math.round(scenario.outlet.choke * 100)} %`);
  set("diam", scenario.segments[0].diameter, `${scenario.segments[0].diameter.toFixed(3)} m`);
  const rough = scenario.options.roughness ?? 4.6e-5;
  set("rough", Math.log10(rough), fmtRough(rough));
  set("speed", Math.log2(speed), fmtSpeed(speed));

  const f = sim.fluid();
  const thermal = sim.thermal();
  $("thermal").checked = thermal;
  $("thermal-ctls").hidden = !thermal;
  set("tin", scenario.inlet.t ?? f.tRef, fmtC(scenario.inlet.t ?? f.tRef));
  set("tamb", f.tAmbient, fmtC(f.tAmbient));
  const u = scenario.options.u_wall ?? 0;
  set("uwall", Math.log10(u + 1), `${u.toFixed(1)} W/m²K`);

  $("fluid").value = scenario.fluid?.preset ?? "air-water";
  $("fluid-note").textContent = `${f.gamma.toFixed(2)} γ · a_gas ${f.aGas.toFixed(0)} m/s`;
  $("fluid-props").innerHTML = [
    ["M gas", `${f.gasMw.toFixed(1)} kg/kmol`],
    ["Z", f.gasZ.toFixed(3)],
    ["ρ liq", `${f.liqRho.toFixed(0)} kg/m³`],
    ["a liq", `${f.liqA.toFixed(0)} m/s`],
    ["μ gas", `${(f.gasMu * 1e6).toFixed(1)} µPa·s`],
    ["μ liq", `${(f.liqMu * 1e3).toFixed(2)} mPa·s`],
    ["σ", `${(f.sigma * 1e3).toFixed(1)} mN/m`],
    ["T ref", fmtC(f.tRef)],
  ]
    .map(([k, v]) => `<span>${k}</span><b>${v}</b>`)
    .join("");

  $("muscl").checked = scenario.options.muscl !== false;
  $("feedback").checked = !!scenario.options.regime_feedback;
  syncRunButton();
}

$("fluid").addEventListener("change", () => {
  scenario.fluid = { preset: $("fluid").value };
  rebuild();
});

$("thermal").addEventListener("change", () => {
  scenario.options.thermal = $("thermal").checked;
  rebuild();
});

$("muscl").addEventListener("change", () => {
  scenario.options.muscl = $("muscl").checked;
  sim?.set_muscl($("muscl").checked);
  pushHash();
});

$("feedback").addEventListener("change", () => {
  scenario.options.regime_feedback = $("feedback").checked;
  rebuild(); // a physics change is deliberate: restart clean
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

$("settle").addEventListener("click", () => {
  if (!sim) return;
  if (reviewing) resumeHere();
  running = true;
  settleUntil = sim.time() + 3600;
  settleRef = null;
  syncRunButton();
});

$("export").addEventListener("click", exportCsv);

for (const b of document.querySelectorAll("#tabs button")) {
  b.addEventListener("click", () => {
    activeView = b.dataset.view;
    for (const o of document.querySelectorAll("#tabs button")) o.classList.toggle("on", o === b);
    for (const v of document.querySelectorAll(".view")) {
      v.classList.toggle("on", v.id === `view-${activeView}`);
    }
    sizeCanvases();
    drawAll();
  });
}

addEventListener("keydown", (e) => {
  if (e.target.matches("input, textarea, select")) return;
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

// ---------- export ----------

function exportCsv() {
  const view = currentView();
  if (!view) return;
  const x = geom.x;
  const elev = Array.from(sim.elev());
  const d = Array.from(sim.diam());
  const head = "x_m,elevation_m,diameter_m,p_Pa,T_K,alpha_gas,holdup_liquid,v_gas_mps,v_liq_mps,regime";
  const rows = [head];
  for (let i = 0; i < view.n; i++) {
    rows.push(
      [
        x[i].toFixed(4),
        elev[i].toFixed(4),
        d[i].toFixed(4),
        view.p[i].toFixed(1),
        view.t[i].toFixed(3),
        view.alpha[i].toFixed(6),
        (1 - view.alpha[i]).toFixed(6),
        view.vg[i].toFixed(4),
        view.vl[i].toFixed(4),
        REGIME_SHORT[view.regime[i]],
      ].join(","),
    );
  }
  const name = `${scenario.name || "phase-flow"}-t${view.time.toFixed(2)}s.csv`;
  const url = URL.createObjectURL(new Blob([rows.join("\n")], { type: "text/csv" }));
  const a = document.createElement("a");
  a.href = url;
  a.download = name;
  a.click();
  URL.revokeObjectURL(url);
}

// ---------- presets & legend ----------

for (const name of Object.keys(PRESETS)) {
  const b = document.createElement("button");
  b.className = "preset";
  b.textContent = name;
  b.addEventListener("click", () => {
    document.querySelectorAll(".preset").forEach((x) => x.classList.remove("on"));
    b.classList.add("on");
    running = true;
    loadScenario(PRESETS[name]);
    syncRunButton();
  });
  $("presets").append(b);
}

for (const [name, blurb] of Object.entries(FLUIDS)) {
  const o = document.createElement("option");
  o.value = name;
  o.textContent = name;
  o.title = blurb;
  $("fluid").append(o);
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
    const fast = settleUntil > 0 ? 256 : speed;
    const t0 = performance.now();
    try {
      sim.step(wallDt * 1000 * fast * budget);
    } catch (e) {
      banner(String(e));
      running = false;
      settleUntil = -1;
      syncRunButton();
    }
    const cost = performance.now() - t0;
    budget = cost > 22 ? Math.max(0.04, budget * 0.7) : Math.min(1, budget * 1.06);
    if (settleUntil > 0) checkSettled();
    if (now - lastCapture >= CAPTURE_MS) capture(now);
  }
  drawAll();
}

/// "Run to steady" stops when the inventory and the boundary rates stop
/// moving — not on a fixed time, because a 2 km line and a 10 m riser settle
/// three orders of magnitude apart.
function checkSettled() {
  const r = sim.report();
  const now = sim.time();
  const liq = r.liquidInventory;
  if (!settleRef || now - settleRef.t > 2.0) {
    if (settleRef) {
      const drift = Math.abs(liq - settleRef.liq) / Math.max(liq, 1e-9) / (now - settleRef.t);
      const imb = Math.max(
        rel(r.wGasIn, r.wGasOut),
        rel(r.wLiqIn, r.wLiqOut),
      );
      if (drift < 2e-4 && imb < 0.01) {
        settleUntil = -1;
        running = false;
        syncRunButton();
        return;
      }
    }
    settleRef = { t: now, liq };
  }
  if (now > settleUntil) {
    settleUntil = -1;
    banner("run to steady: gave up after 3600 s of simulated time");
  }
}

function rel(a, b) {
  const s = Math.max(Math.abs(a), Math.abs(b));
  return s < 1e-9 ? 0 : Math.abs(a - b) / s;
}

function drawAll() {
  const view = currentView();
  if (!view) return;
  pipe.draw(view);
  if (activeView === "profiles") profile.draw(view, geom);
  else if (activeView === "trends") {
    chartP.draw(tape, probes, playhead);
    chartH.draw(tape, probes, playhead);
  } else tdmap.draw(view, scenario.segments[0].diameter, sim);
  readout.update(view.report, { thermal: sim.thermal() });
  updateLegend(view);
  $("t-now").textContent = view.time.toFixed(3);
  $("dt").textContent = view.dt ? `${view.dt.toExponential(1)} s` : "—";
  $("lag").textContent =
    settleUntil > 0
      ? "settling…"
      : budget < 0.99 && running && !reviewing
        ? `lagging ×${budget.toFixed(2)}`
        : "";
}

function banner(msg) {
  const b = $("banner");
  b.style.display = msg ? "block" : "none";
  b.textContent = msg ? String(msg).replace(/^Error:\s*/, "") : "";
}

function sizeCanvases() {
  for (const cv of document.querySelectorAll("canvas")) {
    const r = cv.getBoundingClientRect();
    if (r.width < 1 || r.height < 1) continue; // hidden tab: size it when shown
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
  tdmap = new TDMap($("tdmap"));
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
