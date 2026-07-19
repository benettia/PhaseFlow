// phase-flow panel: boots the wasm solver, owns the loop and the controls.
import init, { WasmSim, classify_point } from "../pkg/phase_flow_wasm.js";
import { PRESETS, encodeHash, decodeHash } from "./presets.js";
import { PipeView } from "./render.js";
import { StripChart } from "./charts.js";
import { TDMap } from "./tdmap.js";

const $ = (id) => document.getElementById(id);

let sim = null;
let scenario = null;
let running = false;
let speed = 1;
let budgetScale = 1;
let probes = [0, 0, 0];
let slamT = -1;

const pipe = new PipeView($("pipe"), onGeometryEdit);
const chartP = new StripChart($("chart-p"), "pressure at probes", " kPa", ["#1c1b18", "#a4442c", "#6b665c"], 1e-3);
const chartH = new StripChart($("chart-h"), "liquid holdup at probes", "", ["#1f4e79", "#a4442c", "#6b665c"]);
let tdmap = null;

function sizeCanvases() {
  for (const cv of document.querySelectorAll("canvas")) {
    const r = cv.getBoundingClientRect();
    cv.width = Math.max(50, r.width * devicePixelRatio | 0);
    cv.height = Math.max(50, r.height * devicePixelRatio | 0);
  }
  pipe.fit();
}
addEventListener("resize", sizeCanvases);

function banner(msg) {
  const b = $("banner");
  b.style.display = msg ? "block" : "none";
  b.textContent = msg || "";
}

function loadScenario(sc, { updateHash = true } = {}) {
  scenario = structuredClone(sc);
  try {
    sim = new WasmSim(JSON.stringify(scenario));
    banner("");
  } catch (e) {
    banner(String(e));
    return;
  }
  pipe.setScenario(scenario);
  chartP.reset();
  chartH.reset();
  // probes: 10 %, riser base (min elevation), 95 %
  const n = sim.n_cells();
  probes = [Math.floor(0.1 * n), sim.min_elev_cell(), Math.floor(0.95 * n)];
  syncControls();
  if (updateHash) history.replaceState(null, "", "#" + encodeHash(scenario));
  drawAll();
}

function onGeometryEdit() {
  scenario.segments = pipe.segments();
  loadScenario(scenario);
}

// --- controls ---
function slider(id, fmt, apply) {
  const el = $(id), out = $(id + "-out");
  el.addEventListener("input", () => {
    out.textContent = fmt(el.value);
    apply(parseFloat(el.value));
  });
  return { el, out, fmt };
}

// mass-rate sliders are logarithmic: value 0..1 -> 10^(lo + v*(hi-lo))
const wgMap = (v) => (v <= 0 ? 0 : 10 ** (-4 + v * 3.3)); // 0 .. ~0.2 kg/s ... 2
const wlMap = (v) => (v <= 0 ? 0 : 10 ** (-2 + v * 4)); // 0.01 .. 100 kg/s
const wgInv = (w) => (w <= 0 ? 0 : (Math.log10(w) + 4) / 3.3);
const wlInv = (w) => (w <= 0 ? 0 : (Math.log10(w) + 2) / 4);

const sWg = slider("wg", (v) => wgMap(v).toPrecision(2) + " kg/s", (v) => bc({ wg: wgMap(v) }));
const sWl = slider("wl", (v) => wlMap(v).toPrecision(2) + " kg/s", (v) => bc({ wl: wlMap(v) }));
const sPo = slider("pout", (v) => v + " bar", (v) => bc({ p: v * 1e5 }));
const sCh = slider("choke", (v) => Math.round(v * 100) + " %", (v) => bc({ choke: v }));
slider("diam", (v) => (+v).toFixed(3) + " m", (v) => {
  pipe.setDiameter(v);
  onGeometryEdit();
});
$("speed").addEventListener("input", () => {
  speed = 2 ** parseFloat($("speed").value);
  $("speed-out").textContent = (speed >= 1 ? speed.toFixed(0) : speed.toFixed(2)) + "×";
});

function bc(part) {
  if (!scenario) return;
  if ("wg" in part) scenario.inlet.wg = part.wg;
  if ("wl" in part) scenario.inlet.wl = part.wl;
  if ("p" in part) scenario.outlet.p = part.p;
  if ("choke" in part) scenario.outlet.choke = part.choke;
  pushBc();
  history.replaceState(null, "", "#" + encodeHash(scenario));
}

function pushBc() {
  if (sim) sim.set_bc(scenario.inlet.wg, scenario.inlet.wl, scenario.outlet.p, scenario.outlet.choke);
}

function syncControls() {
  sWg.el.value = wgInv(scenario.inlet.wg);
  sWg.out.textContent = scenario.inlet.wg.toPrecision(2) + " kg/s";
  sWl.el.value = wlInv(scenario.inlet.wl);
  sWl.out.textContent = scenario.inlet.wl.toPrecision(2) + " kg/s";
  sPo.el.value = scenario.outlet.p / 1e5;
  sPo.out.textContent = (scenario.outlet.p / 1e5).toFixed(1) + " bar";
  sCh.el.value = scenario.outlet.choke;
  sCh.out.textContent = Math.round(scenario.outlet.choke * 100) + " %";
  $("diam").value = scenario.segments[0].diameter;
  $("diam-out").textContent = scenario.segments[0].diameter.toFixed(3) + " m";
  $("muscl").checked = scenario.options?.muscl !== false;
  $("feedback").checked = !!scenario.options?.regime_feedback;
}

$("muscl").addEventListener("change", () => {
  scenario.options = scenario.options || {};
  scenario.options.muscl = $("muscl").checked;
  sim?.set_muscl($("muscl").checked);
});
$("feedback").addEventListener("change", () => {
  scenario.options = scenario.options || {};
  scenario.options.regime_feedback = $("feedback").checked;
  loadScenario(scenario); // physics change is deliberate: restart
});

$("run").addEventListener("click", () => {
  running = !running;
  $("run").textContent = running ? "pause" : "run";
});
$("stepbtn").addEventListener("click", () => {
  if (!sim) return;
  try {
    const dt = sim.single_step();
    $("status").textContent = `t = ${sim.time().toFixed(3)} s · dt = ${dt.toExponential(1)}`;
    drawAll();
  } catch (e) { banner(String(e)); }
});
$("slam").addEventListener("click", () => {
  slamT = sim ? sim.time() : -1;
});

// preset buttons
for (const name of Object.keys(PRESETS)) {
  const b = document.createElement("button");
  b.className = "preset";
  b.textContent = name;
  b.addEventListener("click", () => {
    document.querySelectorAll(".preset").forEach((x) => x.classList.remove("active"));
    b.classList.add("active");
    running = true;
    $("run").textContent = "pause";
    loadScenario(PRESETS[name]);
  });
  $("presets").append(b);
}

// --- main loop ---
let lastT = 0;
function frame(now) {
  requestAnimationFrame(frame);
  const wallDt = Math.min(0.05, (now - lastT) / 1000 || 0.016);
  lastT = now;
  if (sim && running) {
    // valve slam script: choke 100 -> 0 over 0.1 s of sim time
    if (slamT >= 0) {
      const f = Math.max(0, 1 - (sim.time() - slamT) / 0.1);
      scenario.outlet.choke = f;
      pushBc();
      syncControls();
      if (f <= 0) slamT = -1;
    }
    const t0 = performance.now();
    try {
      sim.step(wallDt * 1000 * speed * budgetScale);
    } catch (e) {
      banner(String(e));
      running = false;
      $("run").textContent = "run";
    }
    const cost = performance.now() - t0;
    // keep solver under ~60 % of the frame; recover slowly
    budgetScale = cost > 24 ? Math.max(0.05, budgetScale * 0.7) : Math.min(1, budgetScale * 1.05);
    const p = sim.p(), a = sim.alpha();
    chartP.push(sim.time(), probes.map((i) => p[i]));
    chartH.push(sim.time(), probes.map((i) => 1 - a[i]));
    $("status").textContent =
      `t = ${sim.time().toFixed(2)} s · dt = ${sim.dt_last().toExponential(1)}` +
      (budgetScale < 0.99 ? ` · lagging ×${budgetScale.toFixed(2)}` : "");
  }
  drawAll();
}

function drawAll() {
  if (!sim) return;
  sim.refresh_regime();
  pipe.draw(sim, sim.time());
  chartP.draw();
  chartH.draw();
  tdmap.draw(sim);
}

// --- boot ---
init().then(() => {
  tdmap = new TDMap($("tdmap"), classify_point);
  sizeCanvases();
  const fromHash = location.hash.length > 1 ? decodeHash(location.hash.slice(1)) : null;
  if (fromHash) {
    loadScenario(fromHash, { updateHash: false });
  } else {
    document.querySelector(".preset").classList.add("active");
    loadScenario(PRESETS["water faucet"]);
  }
  running = true;
  $("run").textContent = "pause";
  window.PHASEFLOW_READY = true;
  requestAnimationFrame(frame);
});
