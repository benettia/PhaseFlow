// Preset scenarios (the JSON schema of crates/scenario) + URL-hash codec.
//
// The first four are the verification cases — they are the *same* scenarios
// the acceptance suite runs, so what you see in the browser is what the tests
// assert. The last two are design cases: real fluids, real heat transfer,
// nothing to compare against but engineering judgement.

export const FLUIDS = {
  "air-water": "air / water, 15 °C — the pair every correlation was fitted to",
  "gas-oil": "lean gas / medium crude, 40 °C — a production flowline",
  "gas-condensate": "rich gas / condensate, 60 °C — low surface tension",
};

export const PRESETS = {
  "water faucet": {
    name: "water-faucet",
    segments: [{ length: 12, angle: -90, diameter: 1.0, cells: 96 }],
    init: { alpha_g: 0.2, p: 1.0e5, v: 10 },
    inlet: { wg: 0, wl: 6277.6, p_anchor: 1.0e5, makeup_alpha: 0.2 },
    outlet: { p: 1.0e5, choke: 1, cv: 20 },
    options: { wall_friction: false, fixed_dt: 5e-4 },
    fluid: { preset: "air-water" },
  },
  "gas kick": {
    name: "gas-kick",
    segments: [{ length: 30, angle: 90, diameter: 0.12, cells: 150, init: { alpha_g: 0.02, p: 3e5, v: 0 } }],
    init: { alpha_g: 0.02, p: 3e5, v: 0 },
    inlet: { wg: 0.03, wl: 0.2 },
    outlet: { p: 1.0e5, choke: 1, cv: 0.8 },
    options: { hydrostatic_init: true },
    fluid: { preset: "air-water" },
  },
  "severe slugging": {
    name: "severe-slugging",
    segments: [
      { length: 80, angle: -4, diameter: 0.08, cells: 150, init: { alpha_g: 0.6, p: 2e5, v: 0 } },
      { length: 12, angle: 90, diameter: 0.08, cells: 45, init: { alpha_g: 0.03, p: 2e5, v: 0 } },
    ],
    init: { alpha_g: 0.5, p: 2e5, v: 0 },
    inlet: { wg: 0.003, wl: 2.0 },
    outlet: { p: 1.0e5, choke: 1, cv: 0.6 },
    options: { hydrostatic_init: true, regime_feedback: true },
    fluid: { preset: "air-water" },
  },
  "valve slam": {
    name: "valve-slam",
    segments: [{ length: 100, angle: 0, diameter: 0.1, cells: 200 }],
    init: { alpha_g: 0.1, p: 2e5, v: 1 },
    inlet: { wg: 1.6e-3, wl: 7.07 },
    outlet: { p: 1.9e5, choke: 1, cv: 1.5 },
    options: {},
    fluid: { preset: "air-water" },
  },
  // --- design cases ---
  "wet gas line": {
    name: "wet-gas-line",
    // A buried export line over rolling terrain: condensate collects in the
    // dips, the gas cools toward ground temperature, and the low spots are
    // where a pig would find its liquid.
    segments: [
      // one initial state for the whole line: a per-segment init would put
      // steps in holdup and temperature at every elbow that take a full
      // residence time to advect out, and they look like physics
      { length: 600, angle: -1.2, diameter: 0.25, cells: 70 },
      { length: 400, angle: 2.0, diameter: 0.25, cells: 46 },
      { length: 500, angle: -1.6, diameter: 0.25, cells: 58 },
      { length: 300, angle: 3.0, diameter: 0.25, cells: 36 },
    ],
    init: { alpha_g: 0.93, p: 4.0e6, v: 3, t: 322 },
    inlet: { wg: 9.0, wl: 1.6, t: 333 },
    outlet: { p: 3.8e6, choke: 1, cv: 30 },
    options: {
      hydrostatic_init: true,
      thermal: true,
      t_ambient: 279,
      u_wall: 4.0,
      roughness: 4.6e-5,
    },
    fluid: { preset: "gas-condensate" },
  },
  blowdown: {
    name: "blowdown",
    // Gas line vented to atmosphere: the expansion does work on the fluid
    // leaving, so what stays behind cools — the transient that sets the
    // minimum design metal temperature. Two honest caveats: a constant-Z gas
    // has no Joule-Thomson effect, so this is p dV cooling only, and the
    // pipe wall's own thermal mass is not modelled, so the early cooling
    // rate is an upper bound.
    segments: [{ length: 400, angle: 0, diameter: 0.2, cells: 160, init: { alpha_g: 0.999, p: 8.0e6, v: 0, t: 300 } }],
    init: { alpha_g: 0.999, p: 8.0e6, v: 0, t: 300 },
    inlet: { wg: 0, wl: 0 },
    outlet: { p: 1.0e5, choke: 0.06, cv: 4 },
    options: { thermal: true, t_ambient: 288, u_wall: 8.0 },
    fluid: { preset: "gas-oil" },
  },
};

export function encodeHash(scenario) {
  const json = JSON.stringify(scenario);
  return btoa(String.fromCharCode(...new TextEncoder().encode(json)))
    .replaceAll("+", "-").replaceAll("/", "_").replace(/=+$/, "");
}

export function decodeHash(hash) {
  try {
    const b64 = hash.replaceAll("-", "+").replaceAll("_", "/");
    const bytes = Uint8Array.from(atob(b64), (c) => c.charCodeAt(0));
    return JSON.parse(new TextDecoder().decode(bytes));
  } catch {
    return null;
  }
}
