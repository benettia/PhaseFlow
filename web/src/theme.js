// One palette, shared by every view. Engineering instrument: cream ground,
// ink lines, one accent per phase.

export const T = {
  cream: "#f4efe4",
  panel: "#efe8d8",
  ink: "#1c1b18",
  ink2: "#6b665c",
  ink3: "#a8a091",
  line: "#cdc5b4",
  grid: "#ddd5c3",

  gas: "#dfb45f",
  gasLight: "#f2e2b6",
  gasDeep: "#b5872c",

  liq: "#1f4e79",
  liqLight: "#4f80ac",
  liqDeep: "#0f3355",

  probe: ["#1c1b18", "#a4442c", "#2f7d6e"],
  warn: "#a4442c",
};

// Regime codes match phase_core::Regime.
export const REGIME_NAMES = [
  "stratified smooth",
  "stratified wavy",
  "intermittent (slug)",
  "annular",
  "dispersed bubble",
  "bubbly",
  "churn",
  "single liquid",
  "single gas",
];

export const REGIME_SHORT = [
  "stratified",
  "wavy",
  "slug",
  "annular",
  "dispersed",
  "bubbly",
  "churn",
  "liquid",
  "gas",
];

export const REGIME_COLORS = [
  "#a9bccd", // stratified smooth
  "#8aa6bf", // stratified wavy
  "#5d81a6", // intermittent
  "#e0bd76", // annular
  "#3a6791", // dispersed bubble
  "#4c7099", // bubbly
  "#8d87bd", // churn
  "#1f4e79", // single liquid
  "#e5c98d", // single gas
];

/// Mix two hex colours; t = 0 gives a, t = 1 gives b.
export function mix(a, b, t) {
  const pa = [1, 3, 5].map((i) => parseInt(a.slice(i, i + 2), 16));
  const pb = [1, 3, 5].map((i) => parseInt(b.slice(i, i + 2), 16));
  const c = pa.map((v, i) => Math.round(v + (pb[i] - v) * t));
  return `rgb(${c[0]},${c[1]},${c[2]})`;
}

export function rgba(hex, a) {
  const p = [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16));
  return `rgba(${p[0]},${p[1]},${p[2]},${a})`;
}

/// Crisp text helper: canvases are drawn at devicePixelRatio scale.
export function setFont(ctx, px, weight = "") {
  ctx.font = `${weight} ${px}px ui-monospace, "SF Mono", Menlo, Consolas, monospace`.trim();
}
