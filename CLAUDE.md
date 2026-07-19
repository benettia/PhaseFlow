# CLAUDE.md — working agreement for agents on phase-flow

This file is the hand-off brain of the repo. Read it before touching code.

---

## ⚠️ VERY IMPORTANT RULE — THE CHANGELOG DISCIPLINE

**Every PR MUST update the changelog at the bottom of this file.** Not just
*what* changed — *what you learned*: dead ends you hit, invariants you almost
broke, tolerances you touched and why, anything the next agent would otherwise
have to rediscover the hard way. A PR that changes behavior without a
changelog entry here is incomplete and must not be merged. Treat this file as
append-mostly: correct entries if they turn out wrong, never silently delete
the considerations of previous agents.

Entry format (append newest at the top of the Changelog section):

```
### YYYY-MM-DD — <branch or PR title> (<agent/author>)
- What changed:
- Why / what was tried and rejected:
- Considerations for future agents:
```

---

## What this project is

A transient 1-D drift-flux multiphase pipe flow simulator (isothermal, three
conserved fields per cell: gas mass, liquid mass, mixture momentum), with:

- `crates/core` — the solver. **Zero deps, `#![forbid(unsafe_code)]`, f64
  everywhere, no `mul_add`** (bit-reproducibility across native/wasm is the
  goal; libm still differs in the last ulp — documented, don't chase it).
- `crates/scenario` — JSON ⇄ `phase_core::Scenario`. serde is quarantined
  here so core never grows a dependency.
- `crates/wasm` — wasm-bindgen wrapper. Getters return `Float64Array` views
  into wasm linear memory (zero-copy); the JS side re-acquires views every
  frame because memory growth invalidates them.
- `bindings/python` — pyo3 + maturin, module `phase_flow`. Arrays out as
  numpy **copies** (never aliased mutable views). Keep `phase_flow.pyi` in
  sync with the API.
- `web/` — index.html + ES modules + canvas. No framework, no bundler.
  `web/pkg/` is wasm-pack output.
- `analysis/` — Python verification studies; each file is both a
  plot-producing script (`uv run analysis/<name>.py` → `analysis/out/`) and
  a pytest module (patterns registered in root `pyproject.toml`).

## Build & test chain (memorize this order)

```sh
export PATH="$HOME/.cargo/bin:$PATH"    # cargo/wasm-pack are NOT on default PATH here

cargo test -p phase-core --release      # 1. fast gate (~1 s). NOT workspace-wide:
                                        #    the pyo3 crate can't link under plain `cargo test`
wasm-pack build crates/wasm --target web --release --out-dir ../../web/pkg
                                        # 2. regenerate web/pkg after ANY core change
uv sync --reinstall-package phase-flow  # 3. rebuild python bindings — plain `uv sync`
                                        #    will NOT notice Rust source changes
uv run pytest                           # 4. full verification (~1 min, includes
                                        #    headless-chromium web smoke)
```

Linting/formatting is pre-commit-managed: `uv run pre-commit install` once,
then commits run ruff (check+format), rustfmt, and hygiene hooks. CI
(`.github/workflows/ci.yml`) re-runs all of it plus `clippy -D warnings` and
the full pytest suite, and **fails any PR that does not touch CLAUDE.md**
(the changelog-discipline job). `deploy.yml` publishes `web/` to GitHub Pages
on push to main, rebuilding the wasm pkg from source (`web/pkg/` is
gitignored — never commit it, never hand-edit it).

If you change solver behavior and only rebuild one of wasm/python, the other
wrapper silently keeps the old physics. Always rebuild both.

## Invariants — do not break these

1. **`crates/core` stays zero-dep and unsafe-free.** JSON, FFI, numpy: all
   belong in the wrapper crates.
2. **Conservation is structural.** Conserved masses are never mutated during
   primitive recovery (α clamping happens on derived values only). The
   1e-12/1000-steps test guards this; if you add a source term or a floor
   that touches `mg`/`ml`, you must show the test still passes.
3. **Determinism.** No randomness (splitmix64 if noise is ever needed), no
   iteration counts that depend on convergence checks (Newton polish is a
   fixed 2 iterations; TD bisection a fixed 48), no `mul_add`, no threads.
   Fixed-dt mode must stay bit-identical run-to-run (`f64::to_bits` test).
4. **NaN policy: stop and name the cell.** Never render or return garbage.
   `compute_prim` is the checkpoint.
5. **Single-phase limits of the slip law are exact.** `C0 = 1 + 0.2(1−α²)²`
   — the *squared* profile term is deliberate: C0−1 must vanish faster than
   (1−α) or the liquid limps at 0.6·v_g in the pure-gas limit (this was a
   real bug: a spurious spike rode every shock front). `1 − C0·α > 0` on
   (0,1) is required by the velocity 2×2 solve — check both properties if
   you touch `closures::c0`.
6. **CFL default is 0.5, not 0.9.** Heun + AUSMV at CFL 0.9 is marginally
   dispersive at shocks: 3.7 % shock-speed error, vs 0.08 % at 0.5. Measured,
   not folklore (the acceptance test will catch a regression).
7. **The physics must stay emergent.** Severe slugging comes from the
   stratified-C0 regime feedback, never from scripting. If a preset needs a
   hack to look right, the model is wrong — fix the model or document the
   limitation honestly.

## Hard-won knowledge (read before "fixing" these)

- **Ransom faucet has a model-error floor of L1 ≈ 0.06** in strict
  drift-flux: pressure equilibrates at the Wood speed (~25 m/s at α = 0.2),
  so the column cannot stay isobaric as the two-fluid analytic solution
  assumes. Do not "tune" the solver to close this gap — it is the model, and
  the spec forbids upgrading to two-fluid. Numerics are verified separately
  by Cauchy self-convergence (order ≥ 0.8 on grids 48/96/192).
- **Faucet inlet is a pressure-anchored gas make-up feed** (`p_anchor` +
  `makeup_alpha`): an open faucet top where air replaces the liquid that
  falls away. Two alternatives were tried and failed: fixed wg + p-anchor
  (violent boundary blow-up: over-determined), and extrapolated-p mass-flux
  inlet (the column hangs on a suction gradient and de-gasses from the top).
  A τ-filtered make-up velocity was also tried to improve fine-grid
  convergence and made it *worse* (resonant lag) — reverted. Faucet Cauchy
  convergence saturates beyond N ≈ 200 on inlet-boundary-layer noise; this
  is known, visible in `analysis/out/faucet.png`, and asserted around.
- **Severe slugging does not emerge from plain Zuber–Findlay slip. Period.**
  With any uniform C0 ≥ 0.5 the pipeline gas rides the mixture and the
  system finds a stable steady state (weeks of parameter scanning won't help
  — we scanned). The mechanism requires `regime_feedback: true`: stratified
  cells blend C0 → 0 (`C0_STRAT` in `sim.rs`, α-blended by `4α(1−α)` to keep
  single-phase limits exact), so gas stalls against the draining liquid,
  accumulates, and the stratified → intermittent regime flip is the surge
  trigger. Validated cycle: 80 m @ −4° + 12 m riser, D = 0.08 m, wl = 2.0,
  wg = 0.003 kg/s → period 144.4 s ± 0.1 %. The regime array used by the
  feedback is lagged (refreshed every 16 steps) — that is fine and
  deterministic; don't move classification into the per-stage hot path
  (bisection per cell is too slow there).
- **Shock-speed measurement**: single-threshold front detection has a ±1-cell
  bias; measure front *displacement between two times* (the tests do).
- **Boundary conditions live in the flux**, not in ghost cells. Inlet with
  both rates zero degenerates to a pressure-only wall flux — that is exactly
  a closed end, used by the conservation test. Outlet choke ≤ 1e-4 is a wall.

## Environment quirks (this machine)

- `cargo`/`wasm-pack`: `~/.cargo/bin` (not on non-interactive PATH). `uv`
  is on PATH via snap.
- System Python is externally-managed; playwright chromium cache is shared
  between the user-site install and the uv venv (`~/.cache/ms-playwright`).
- pyo3/numpy crates pinned at 0.25/0.25 (matched pair, abi3-py310).

## Style

- Match the existing voice: comments state constraints the code can't show
  (why C0 is squared, why a tolerance is what it is), not narration.
- Solver code: SoA `Vec<f64>`, no allocations in the step path (scratch
  buffers live on `Sim`).
- Web: ES modules, no deps, cream/ink instrument aesthetic (`--cream`,
  `--ink`, amber gas, blue liquid). No dashboards, no glassmorphism.
- Tolerances in tests are engineering statements: if you loosen one, the
  changelog entry must say by how much and why.

---

## Changelog

### 2026-07-20 — timeline rollback + visualization overhaul (Claude, with benettia)
- What changed: solver gained `save_state`/`load_state` (flat
  `[mg|ml|mom|regime]`, exposed through both wrappers) and a test proving a
  restored snapshot reproduces the continuation bit-for-bit under adaptive
  CFL. Web app rebuilt around a recorded tape (`web/src/history.js`): scrub
  to rewind every view together, press run to roll the solver back to that
  frame and branch. Renderer rewritten (shared-edge quads, continuous
  stratified free surface, globally-phased advected patterns, per-segment
  cylindrical shading, probe markers); charts now read from the tape with a
  playhead; TD map got named regions; new `web/src/theme.js` holds the one
  palette. Web smoke test now exercises the rollback.
- Why / what was tried and rejected: the regime array **must** be part of the
  snapshot — with `regime_feedback` on it is lagged state, not a function of
  the masses — and so must `steps`, because the regime refresh runs on a step
  cadence; without either, a restored run silently diverges. Four separate
  causes of "barcode" striping down the pipe were fixed in order: (1) AA
  seams between abutting quads → overlap them; (2) a *symmetric* overlap made
  each cell repaint its neighbour's near end, punching gas slivers through
  the liquid wherever holdup stepped → overlap forward only, since cells are
  drawn in order; (3) art overhang was a fixed 1.5 px while the clip inflated
  by 0.75·dpr, so at dpr = 3 the base colour showed through → scale the
  overhang with dpr; (4) per-cell wave/bubble phases (`sin(... + 3*i)`) reset
  at every boundary → phase everything on absolute pipe position. Also
  removed the render loop's `refresh_regime()` call: with regime feedback on
  it made the trajectory depend on frame rate.
- Considerations for future agents: `drawStratified` runs **before** the
  per-cell art and spills a few px past each end on purpose — mitered elbow
  faces otherwise leave a wedge of the bent end cell uncovered; cells drawn
  afterwards reclaim their own territory. If you add a regime whose art fills
  the whole cell, check it against a neighbour with different holdup at
  dpr = 3 before believing it is seam-free (`device_scale_factor=3` in a
  playwright clip screenshot is how all four bugs above were caught —
  none were visible at dpr = 1). The tape is memory-budgeted (~24 MB), so
  frame count falls as cell count rises; don't assume a fixed window.

### 2026-07-19 — outlet backflow admission + gas-kick verification (Claude)
- What changed: outlet BC now admits reservoir fluid through the choke when
  the last cell's flow reverses (smoothly gated: zero for v ≥ 0, full by
  v = −0.1·c); new `SimError::BlowupAtCell` velocity guard (|v| > 1e4 m/s
  stops the sim naming the cell); new `analysis/gas_kick.py` verification
  (migration, expansion-driven front acceleration 1.3 → 2.6 m/s, unloading);
  shock-tube plot now overlays first-order vs MUSCL; crate metadata; CI
  caches playwright browsers.
- Why / what was tried and rejected: the gas-kick preset stalled at t ≈ 22 s
  — after unloading, liquid falls back from the top cell, the ψ⁺-only outlet
  admits nothing, the cell drains to vacuum (p → floor), and finite momentum
  over floored mass gives v ~ 1e9 → dt ~ 1e-10 → MaxSubsteps hang.
  *Ungated* ψ⁻ admission fixed the vacuum but injected reservoir mass during
  ordinary subsonic outflow too (ψ⁻ ≠ 0 for all v < c) and dragged faucet
  self-convergence to 0.79 — hence the reversal gate. Slugging re-validated
  after the change: 145.4/144.9/145.4 s (was 144.4 ±0.1 %; ~1 s shift from
  reservoir gas re-entry during fallback is real physics, not drift).
- Considerations for future agents: any outlet-BC change must re-run BOTH
  the faucet convergence AND the slugging cycle — they pull in opposite
  directions (clean outflow vs reversal robustness). The gas-kick top cell
  keeps a small α blip (outlet boundary layer, cosmetic); `front_position()`
  in gas_kick.py deliberately measures the bottom-connected front to ignore
  it.

### 2026-07-19 — ci fix: build wasm pkg before the web smoke (Claude)
- What changed: the python CI job now installs the wasm32 target + wasm-pack
  and builds `web/pkg` before pytest; `smoke_web.py` fails fast with a clear
  message when the pkg is missing.
- Why: first CI run on GitHub failed only in `test_web_boots` —
  `web/pkg/` is gitignored, so a fresh checkout has no wasm module and the
  page never sets `PHASEFLOW_READY`. Locally it passed because a stale local
  build was present. Classic works-on-my-machine via ignored build output.
- Considerations for future agents: anything that serves `web/` (CI job,
  deploy, local demo) must build the pkg itself; never "fix" this by
  committing `web/pkg/`.

### 2026-07-19 — tooling: pre-commit + CI/CD (Claude, with benettia)
- What changed: `.pre-commit-config.yaml` (ruff check+format, rustfmt,
  whitespace/yaml/toml hygiene, a non-blocking CLAUDE.md reminder);
  `.github/workflows/ci.yml` (rust fmt/clippy/tests, wasm-pack build, ruff +
  full pytest with chromium, and the blocking changelog-discipline job on
  PRs); `.github/workflows/deploy.yml` (GitHub Pages deploy of `web/` on
  main). Ruff config + dev group in root `pyproject.toml`. Codebase brought
  to `cargo fmt` + `clippy -D warnings` + ruff clean.
- Why / what was tried and rejected: clippy flagged the NaN-guarding
  `!(p > P_MIN)` in `eos.rs` — rewritten as `p.is_nan() || p < P_MIN`, which
  is equivalent *including the NaN path*; if you touch that guard, keep the
  NaN branch or the pressure floor stops catching corrupted states. Clippy is
  scoped to the three lintable crates (`-p` flags) because the pyo3 crate
  can't build under plain host `cargo clippy --workspace`.
- Considerations for future agents: the pre-commit CLAUDE.md hook only
  *reminds* (local index state is too noisy to block on); the CI job is the
  enforcement. Pages deploy needs one-time repo setting: Pages → Source →
  "GitHub Actions". Hook revs are pinned (`v5.0.0`, ruff `v0.8.4`); bump with
  `pre-commit autoupdate`, not by hand-editing blindly.

### 2026-07-19 — initial build (Claude, with benettia)
- What changed: everything — workspace scaffolded and built to green:
  `crates/{core,scenario,wasm}`, `bindings/python`, `web/`, `analysis/`.
  9 cargo tests + 5 pytest verification studies passing; all four presets
  live; severe slugging emergent with a 144.4 s ± 0.1 % cycle.
- Why / what was tried and rejected: see "Hard-won knowledge" above — the
  linear C0 profile (broken pure-gas limit), CFL 0.9 (dispersive at shocks),
  three faucet inlet BCs (two rejected), plain-slip severe slugging (never
  cycles), τ-filtered make-up BC (worse convergence, reverted). The debug
  examples used for the hunt were deleted except
  `crates/core/examples/slug_waterfall.rs`, kept as a terminal demo of the
  slugging cycle (`cargo run --release -p phase-core --example slug_waterfall`).
- Considerations for future agents: the wasm and python wrappers do not
  rebuild themselves — stale-physics bugs look like heisenbugs, check build
  freshness first. The slugging pytest is the slow one (~40 s of the ~60 s
  suite); don't "optimize" it by shortening the run below ~3 cycles or the
  period assertion loses meaning. `web/pkg/` is generated output — regenerate
  rather than hand-edit. The line budget in the original spec (~1500) was
  superseded by the multi-crate layout; keep the solver lean anyway (core is
  ~800 lines).
