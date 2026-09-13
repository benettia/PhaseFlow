// Design report: the numbers you size a line from, updated every frame from
// the solver's own `report()` so they can never drift from the picture.
//
// DOM is built once and only text/width is touched afterwards — this runs
// inside the render loop.

const PD = [
  ["friction", "dpFriction", "var(--warn)"],
  ["elevation", "dpGravity", "var(--liq)"],
  ["acceleration", "dpAccel", "var(--ink3)"],
];

export class Readout {
  constructor(root) {
    root.innerHTML = `
      <div class="grp">
        <h3>PRESSURE DROP<span id="ro-dp-total">—</span></h3>
        <div class="bar" id="ro-dp-bar"></div>
        <dl id="ro-dp-list"></dl>
      </div>
      <div class="grp">
        <h3>INVENTORY</h3>
        <dl>
          <dt>liquid</dt><dd id="ro-liq">—</dd>
          <dt>gas</dt><dd id="ro-gas">—</dd>
          <dt>mean holdup</dt><dd id="ro-holdup">—</dd>
        </dl>
        <div class="meter"><i id="ro-holdup-fill"></i></div>
      </div>
      <div class="grp">
        <h3>RATES<span id="ro-balance"></span></h3>
        <dl>
          <dt>gas in / out</dt><dd id="ro-wg">—</dd>
          <dt>liquid in / out</dt><dd id="ro-wl">—</dd>
        </dl>
      </div>
      <div class="grp">
        <h3>CHECKS</h3>
        <dl>
          <dt title="API RP 14E, C = 100">erosional v/v<sub>e</sub></dt><dd id="ro-ero">—</dd>
          <dt>max mixture v</dt><dd id="ro-vmax">—</dd>
          <dt title="sets the surge pressure and the time step">min a<sub>mix</sub></dt><dd id="ro-amin">—</dd>
          <dt>pressure range</dt><dd id="ro-prange">—</dd>
          <dt id="ro-trow-dt">temperature</dt><dd id="ro-trange">—</dd>
        </dl>
        <div class="meter"><i id="ro-ero-fill"></i></div>
      </div>`;
    this.el = (id) => root.querySelector("#" + id);
    const bar = this.el("ro-dp-bar");
    const list = this.el("ro-dp-list");
    this.seg = {};
    this.val = {};
    for (const [label, key, color] of PD) {
      const i = document.createElement("i");
      i.style.background = color;
      i.title = label;
      bar.append(i);
      this.seg[key] = i;
      const dt = document.createElement("dt");
      dt.innerHTML = `<b style="background:${color}"></b>${label}`;
      const dd = document.createElement("dd");
      dd.textContent = "—";
      list.append(dt, dd);
      this.val[key] = dd;
    }
    this.tRow = this.el("ro-trow-dt");
    this.tVal = this.el("ro-trange");
  }

  update(r, { thermal }) {
    if (!r) return;
    const mag = PD.reduce((s, [, k]) => s + Math.abs(r[k]), 0) || 1;
    for (const [, key] of PD) {
      const f = Math.abs(r[key]) / mag;
      this.seg[key].style.flexGrow = String(Math.max(f, 0.001));
      this.val[key].textContent = pressure(r[key]);
      this.val[key].classList.toggle("neg", r[key] < 0);
    }
    this.el("ro-dp-total").textContent = pressure(r.dpTotal);

    this.el("ro-liq").textContent = mass(r.liquidInventory);
    this.el("ro-gas").textContent = mass(r.gasInventory);
    this.el("ro-holdup").textContent = r.holdupAvg.toFixed(4);
    this.el("ro-holdup-fill").style.width = `${(100 * r.holdupAvg).toFixed(1)}%`;

    this.el("ro-wg").textContent = `${rate(r.wGasIn)} / ${rate(r.wGasOut)}`;
    this.el("ro-wl").textContent = `${rate(r.wLiqIn)} / ${rate(r.wLiqOut)}`;
    // "steady" means what goes in comes out; the residual is scaled on the
    // larger of the two so a shut-in line does not read as wildly unsteady
    const imb = Math.max(
      relDiff(r.wGasIn, r.wGasOut),
      relDiff(r.wLiqIn, r.wLiqOut),
    );
    const bal = this.el("ro-balance");
    bal.textContent = imb < 0.02 ? "steady" : `±${(100 * imb).toFixed(0)} %`;
    bal.className = imb < 0.02 ? "tag ok" : "tag";

    const ero = r.erosionRatio;
    this.el("ro-ero").textContent = `${ero.toFixed(2)} @ cell ${r.erosionCell}`;
    this.el("ro-ero").className = ero > 1 ? "bad" : ero > 0.8 ? "warn" : "";
    const fill = this.el("ro-ero-fill");
    fill.style.width = `${Math.min(100, 100 * ero)}%`;
    fill.className = ero > 1 ? "bad" : ero > 0.8 ? "warn" : "";
    this.el("ro-vmax").textContent = `${r.vMixMax.toFixed(2)} m/s @ ${r.vMixMaxCell}`;
    this.el("ro-amin").textContent = `${r.aMin.toFixed(1)} m/s`;
    this.el("ro-prange").textContent =
      `${(r.pMin / 1e5).toFixed(2)} – ${(r.pMax / 1e5).toFixed(2)} bar`;

    this.tRow.style.display = thermal ? "" : "none";
    this.tVal.style.display = thermal ? "" : "none";
    if (thermal) {
      this.tVal.textContent =
        `${(r.tMin - 273.15).toFixed(1)} – ${(r.tMax - 273.15).toFixed(1)} °C`;
    }
  }
}

function relDiff(a, b) {
  const d = Math.abs(a - b);
  const s = Math.max(Math.abs(a), Math.abs(b));
  return s < 1e-9 ? 0 : d / s;
}

function pressure(pa) {
  const a = Math.abs(pa);
  if (a >= 1e5) return `${(pa / 1e5).toFixed(2)} bar`;
  if (a >= 1e3) return `${(pa / 1e3).toFixed(1)} kPa`;
  return `${pa.toFixed(1)} Pa`;
}

function mass(kg) {
  if (kg >= 1e3) return `${(kg / 1e3).toFixed(2)} t`;
  if (kg >= 1) return `${kg.toFixed(1)} kg`;
  return `${(kg * 1e3).toFixed(1)} g`;
}

function rate(w) {
  const a = Math.abs(w);
  if (a >= 100) return w.toFixed(0);
  if (a >= 1) return w.toFixed(2);
  if (a >= 1e-3) return w.toFixed(4);
  return w.toExponential(1);
}
