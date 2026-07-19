// Preset scenarios (the JSON schema of crates/scenario) + URL-hash codec.

export const PRESETS = {
  "water faucet": {
    name: "water-faucet",
    segments: [{ length: 12, angle: -90, diameter: 1.0, cells: 96 }],
    init: { alpha_g: 0.2, p: 1.0e5, v: 10 },
    inlet: { wg: 0, wl: 6283.2, p_anchor: 1.0e5, makeup_alpha: 0.2 },
    outlet: { p: 1.0e5, choke: 1, cv: 20 },
    options: { wall_friction: false, fixed_dt: 5e-4 },
  },
  "gas kick": {
    name: "gas-kick",
    segments: [{ length: 30, angle: 90, diameter: 0.12, cells: 150, init: { alpha_g: 0.02, p: 3e5, v: 0 } }],
    init: { alpha_g: 0.02, p: 3e5, v: 0 },
    inlet: { wg: 0.03, wl: 0.2 },
    outlet: { p: 1.0e5, choke: 1, cv: 0.8 },
    options: { hydrostatic_init: true },
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
  },
  "valve slam": {
    name: "valve-slam",
    segments: [{ length: 100, angle: 0, diameter: 0.1, cells: 200 }],
    init: { alpha_g: 0.1, p: 2e5, v: 1 },
    inlet: { wg: 1.6e-3, wl: 7.07 },
    outlet: { p: 1.9e5, choke: 1, cv: 1.5 },
    options: {},
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
