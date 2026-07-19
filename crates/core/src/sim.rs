//! Finite-volume drift-flux simulator: SoA state, AUSMV fluxes, RK2 (Heun),
//! CFL-adaptive or fixed dt, NaN guard that names the offending cell.

use crate::closures::*;
use crate::eos::*;
use crate::regime::classify;
use crate::scenario::{Options, Scenario};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SimError {
    NanAtCell(usize),
    BlowupAtCell(usize),
    MaxSubsteps,
}

const MASS_FLOOR: f64 = 1.0e-10;
const V_MAX: f64 = 1.0e4; // [m/s] far beyond anything physical in a pipe
const MAX_SUBSTEPS: usize = 200_000;
const REGIME_EVERY: u64 = 16;

/// Primitive fields derived from conserved state.
#[derive(Clone, Default)]
struct Prim {
    p: Vec<f64>,
    alpha: Vec<f64>,
    rho_g: Vec<f64>,
    rho_l: Vec<f64>,
    vg: Vec<f64>,
    vl: Vec<f64>,
    am: Vec<f64>, // Wood mixture sound speed
}

impl Prim {
    fn with_len(n: usize) -> Self {
        Self {
            p: vec![0.0; n],
            alpha: vec![0.0; n],
            rho_g: vec![0.0; n],
            rho_l: vec![0.0; n],
            vg: vec![0.0; n],
            vl: vec![0.0; n],
            am: vec![0.0; n],
        }
    }
}

pub struct Sim {
    pub n: usize,
    // geometry (per cell / per face)
    dx: Vec<f64>,
    diam: Vec<f64>,
    sin_th: Vec<f64>,
    cos_th: Vec<f64>,
    area: Vec<f64>,
    area_face: Vec<f64>, // n+1
    x_mid: Vec<f64>,
    elev: Vec<f64>,
    // conserved state
    mg: Vec<f64>,
    ml: Vec<f64>,
    mom: Vec<f64>,
    prim: Prim,
    regime: Vec<u8>,
    // boundary conditions (live-settable)
    pub wg_in: f64,
    pub wl_in: f64,
    pub p_anchor: Option<f64>,
    pub makeup_alpha: Option<f64>,
    pub p_out: f64,
    pub choke: f64,
    cv: f64,
    pub opts: Options,
    time: f64,
    dt_last: f64,
    steps: u64,
    // scratch
    s_mg: Vec<f64>,
    s_ml: Vec<f64>,
    s_mom: Vec<f64>,
    s_prim: Prim,
    d1: [Vec<f64>; 3],
    d2: [Vec<f64>; 3],
    flux: [Vec<f64>; 3], // n+1 faces
    wl_recon: [Vec<f64>; 5],
    wr_recon: [Vec<f64>; 5],
}

impl Sim {
    pub fn new(sc: &Scenario) -> Result<Self, SimError> {
        let n: usize = sc.segments.iter().map(|s| s.cells).sum();
        assert!((4..=4096).contains(&n), "cell count out of range");
        let mut dx = Vec::with_capacity(n);
        let mut diam = Vec::with_capacity(n);
        let mut sin_th = Vec::with_capacity(n);
        let mut cos_th = Vec::with_capacity(n);
        let mut init = Vec::with_capacity(n);
        for seg in &sc.segments {
            let th = seg.angle_deg.to_radians();
            for _ in 0..seg.cells {
                dx.push(seg.length / seg.cells as f64);
                diam.push(seg.diameter);
                sin_th.push(th.sin());
                cos_th.push(th.cos());
                init.push(seg.init.unwrap_or(sc.init));
            }
        }
        let area: Vec<f64> = diam
            .iter()
            .map(|d| 0.25 * std::f64::consts::PI * d * d)
            .collect();
        let mut area_face = vec![0.0; n + 1];
        area_face[0] = area[0];
        area_face[n] = area[n - 1];
        for i in 1..n {
            area_face[i] = 0.5 * (area[i - 1] + area[i]);
        }
        let mut x_mid = vec![0.0; n];
        let mut elev = vec![0.0; n];
        let (mut x, mut z) = (0.0, 0.0);
        for i in 0..n {
            x_mid[i] = x + 0.5 * dx[i];
            elev[i] = z + 0.5 * dx[i] * sin_th[i];
            x += dx[i];
            z += dx[i] * sin_th[i];
        }
        // initial pressure: as given, optionally replaced by a hydrostatic
        // sweep from the outlet (two fixed-point passes for rho(p))
        let mut p: Vec<f64> = init.iter().map(|s| s.p).collect();
        if sc.options.hydrostatic_init {
            for _ in 0..2 {
                let a_out = init[n - 1].alpha_g;
                p[n - 1] = sc.outlet.p
                    + rho_mix(a_out, p[n - 1]) * sc.options.g * 0.5 * dx[n - 1] * sin_th[n - 1];
                for i in (0..n - 1).rev() {
                    let rm = 0.5
                        * (rho_mix(init[i].alpha_g, p[i]) + rho_mix(init[i + 1].alpha_g, p[i + 1]));
                    p[i] = p[i + 1] + rm * sc.options.g * (elev[i + 1] - elev[i]);
                }
            }
        }
        let mut mg = vec![0.0; n];
        let mut ml = vec![0.0; n];
        let mut mom = vec![0.0; n];
        for i in 0..n {
            let a = init[i].alpha_g.clamp(ALPHA_EPS, 1.0 - ALPHA_EPS);
            mg[i] = a * rho_gas(p[i]);
            ml[i] = (1.0 - a) * rho_liq(p[i]);
            mom[i] = (mg[i] + ml[i]) * init[i].v;
        }
        let mut sim = Sim {
            n,
            dx,
            diam,
            sin_th,
            cos_th,
            area,
            area_face,
            x_mid,
            elev,
            mg,
            ml,
            mom,
            prim: Prim::with_len(n),
            regime: vec![0; n],
            wg_in: sc.inlet.wg,
            wl_in: sc.inlet.wl,
            p_anchor: sc.inlet.p_anchor,
            makeup_alpha: sc.inlet.makeup_alpha,
            p_out: sc.outlet.p,
            choke: sc.outlet.choke,
            cv: if sc.outlet.cv > 0.0 {
                sc.outlet.cv
            } else {
                0.5
            },
            opts: sc.options,
            time: 0.0,
            dt_last: 0.0,
            steps: 0,
            s_mg: vec![0.0; n],
            s_ml: vec![0.0; n],
            s_mom: vec![0.0; n],
            s_prim: Prim::with_len(n),
            d1: [vec![0.0; n], vec![0.0; n], vec![0.0; n]],
            d2: [vec![0.0; n], vec![0.0; n], vec![0.0; n]],
            flux: [vec![0.0; n + 1], vec![0.0; n + 1], vec![0.0; n + 1]],
            wl_recon: std::array::from_fn(|_| vec![0.0; n]),
            wr_recon: std::array::from_fn(|_| vec![0.0; n]),
        };
        // bootstrap: recover primitives with neutral regimes (no stratified
        // feedback), classify, then recover again with the real regimes
        sim.regime.fill(2);
        compute_prim(
            &sim.mg,
            &sim.ml,
            &sim.mom,
            &sim.sin_th,
            &sim.regime,
            &sim.opts,
            &mut sim.prim,
        )?;
        sim.update_regime();
        compute_prim(
            &sim.mg,
            &sim.ml,
            &sim.mom,
            &sim.sin_th,
            &sim.regime,
            &sim.opts,
            &mut sim.prim,
        )?;
        Ok(sim)
    }

    pub fn time(&self) -> f64 {
        self.time
    }
    pub fn dt_last(&self) -> f64 {
        self.dt_last
    }
    pub fn steps(&self) -> u64 {
        self.steps
    }
    pub fn alpha(&self) -> &[f64] {
        &self.prim.alpha
    }
    pub fn p(&self) -> &[f64] {
        &self.prim.p
    }
    pub fn vg(&self) -> &[f64] {
        &self.prim.vg
    }
    pub fn vl(&self) -> &[f64] {
        &self.prim.vl
    }
    pub fn am(&self) -> &[f64] {
        &self.prim.am
    }
    pub fn mg(&self) -> &[f64] {
        &self.mg
    }
    pub fn ml(&self) -> &[f64] {
        &self.ml
    }
    pub fn regime(&self) -> &[u8] {
        &self.regime
    }
    pub fn x_mid(&self) -> &[f64] {
        &self.x_mid
    }
    pub fn elev(&self) -> &[f64] {
        &self.elev
    }
    pub fn diam(&self) -> &[f64] {
        &self.diam
    }
    pub fn area(&self) -> &[f64] {
        &self.area
    }
    pub fn dx(&self) -> &[f64] {
        &self.dx
    }

    /// Total mass of each phase [kg] (for conservation checks).
    pub fn mass_totals(&self) -> (f64, f64) {
        let mut tg = 0.0;
        let mut tl = 0.0;
        for i in 0..self.n {
            tg += self.mg[i] * self.area[i] * self.dx[i];
            tl += self.ml[i] * self.area[i] * self.dx[i];
        }
        (tg, tl)
    }

    pub fn set_bc(&mut self, wg: f64, wl: f64, p_out: f64, choke: f64) {
        self.wg_in = wg.max(0.0);
        self.wl_in = wl.max(0.0);
        self.p_out = p_out.max(P_MIN);
        self.choke = choke.clamp(0.0, 1.0);
    }

    /// Flat snapshot of everything that evolves: `[mg | ml | mom | regime]`,
    /// length 4n. The regime array is carried because with `regime_feedback`
    /// on it is lagged state, not a pure function of the masses — omitting it
    /// would make a restored run diverge from the original.
    pub fn save_state(&self) -> Vec<f64> {
        let mut v = Vec::with_capacity(4 * self.n);
        v.extend_from_slice(&self.mg);
        v.extend_from_slice(&self.ml);
        v.extend_from_slice(&self.mom);
        v.extend(self.regime.iter().map(|&r| r as f64));
        v
    }

    /// Restore a snapshot taken from a `Sim` with identical geometry.
    /// Rollback is exact: the solver is a pure function of (state, steps),
    /// so resuming from a restored snapshot reproduces the trajectory the
    /// original run would have taken (`steps` matters because the regime
    /// refresh is on a fixed step cadence).
    pub fn load_state(&mut self, data: &[f64], time: f64, steps: u64) -> Result<(), SimError> {
        assert_eq!(data.len(), 4 * self.n, "snapshot length mismatch");
        let n = self.n;
        self.mg.copy_from_slice(&data[..n]);
        self.ml.copy_from_slice(&data[n..2 * n]);
        self.mom.copy_from_slice(&data[2 * n..3 * n]);
        for i in 0..n {
            self.regime[i] = data[3 * n + i] as u8;
        }
        self.time = time;
        self.steps = steps;
        self.dt_last = 0.0;
        compute_prim(
            &self.mg,
            &self.ml,
            &self.mom,
            &self.sin_th,
            &self.regime,
            &self.opts,
            &mut self.prim,
        )
    }

    fn cfl_dt(&self) -> f64 {
        let mut dt = f64::MAX;
        for i in 0..self.n {
            let v = self.prim.vg[i].abs().max(self.prim.vl[i].abs());
            let d = self.dx[i] / (v + self.prim.am[i]);
            if d < dt {
                dt = d;
            }
        }
        self.opts.cfl * dt
    }

    /// One time step (Heun / RK2). Returns dt taken.
    pub fn step(&mut self) -> Result<f64, SimError> {
        let dt = match self.opts.fixed_dt {
            Some(f) => f,
            None => self.cfl_dt(),
        };
        // stage 1: U1 = U + dt * L(U)
        self.rhs(true)?; // fills d1 using self.prim (current state)
        for i in 0..self.n {
            self.s_mg[i] = (self.mg[i] + dt * self.d1[0][i]).max(MASS_FLOOR);
            self.s_ml[i] = (self.ml[i] + dt * self.d1[1][i]).max(MASS_FLOOR);
            self.s_mom[i] = self.mom[i] + dt * self.d1[2][i];
        }
        compute_prim(
            &self.s_mg,
            &self.s_ml,
            &self.s_mom,
            &self.sin_th,
            &self.regime,
            &self.opts,
            &mut self.s_prim,
        )?;
        // stage 2: U^{n+1} = U + dt/2 (L(U) + L(U1))
        self.rhs(false)?; // fills d2 using s_* state
        for i in 0..self.n {
            self.mg[i] = (self.mg[i] + 0.5 * dt * (self.d1[0][i] + self.d2[0][i])).max(MASS_FLOOR);
            self.ml[i] = (self.ml[i] + 0.5 * dt * (self.d1[1][i] + self.d2[1][i])).max(MASS_FLOOR);
            self.mom[i] += 0.5 * dt * (self.d1[2][i] + self.d2[2][i]);
        }
        compute_prim(
            &self.mg,
            &self.ml,
            &self.mom,
            &self.sin_th,
            &self.regime,
            &self.opts,
            &mut self.prim,
        )?;
        self.time += dt;
        self.dt_last = dt;
        self.steps += 1;
        if self.steps.is_multiple_of(REGIME_EVERY) {
            self.update_regime();
        }
        Ok(dt)
    }

    /// Advance simulated time by `t` seconds (whole steps; the last step may
    /// overshoot by less than one dt — callers render whatever state exists).
    pub fn advance(&mut self, t: f64) -> Result<(), SimError> {
        let target = self.time + t;
        let mut sub = 0;
        while self.time < target {
            self.step()?;
            sub += 1;
            if sub > MAX_SUBSTEPS {
                return Err(SimError::MaxSubsteps);
            }
        }
        Ok(())
    }

    pub fn update_regime(&mut self) {
        for i in 0..self.n {
            let a = self.prim.alpha[i];
            let jg = a * self.prim.vg[i];
            let jl = (1.0 - a) * self.prim.vl[i];
            self.regime[i] = classify(
                a,
                jg,
                jl,
                self.diam[i],
                self.sin_th[i],
                self.cos_th[i],
                self.prim.rho_g[i],
                self.prim.rho_l[i],
                self.opts.g,
            ) as u8;
        }
    }

    /// Spatial operator L(U) -> d[0..3]. `first_stage` selects state buffers.
    fn rhs(&mut self, first_stage: bool) -> Result<(), SimError> {
        let n = self.n;
        let (mg, ml, mom, pr) = if first_stage {
            (&self.mg, &self.ml, &self.mom, &self.prim)
        } else {
            (&self.s_mg, &self.s_ml, &self.s_mom, &self.s_prim)
        };
        // --- reconstruction: W = [mg, ml, vg, vl, p] ---
        {
            let vars: [&[f64]; 5] = [mg, ml, &pr.vg, &pr.vl, &pr.p];
            for (k, w) in vars.iter().enumerate() {
                let wl = &mut self.wl_recon[k];
                let wr = &mut self.wr_recon[k];
                if self.opts.muscl {
                    for i in 0..n {
                        let s = if i == 0 || i == n - 1 {
                            0.0
                        } else {
                            mc_limiter(w[i] - w[i - 1], w[i + 1] - w[i])
                        };
                        wl[i] = w[i] + 0.5 * s; // left state of face i+1/2
                        wr[i] = w[i] - 0.5 * s; // right state of face i-1/2
                    }
                } else {
                    wl.copy_from_slice(w);
                    wr.copy_from_slice(w);
                }
            }
            // positivity of reconstructed states
            for i in 0..n {
                self.wl_recon[0][i] = self.wl_recon[0][i].max(MASS_FLOOR);
                self.wr_recon[0][i] = self.wr_recon[0][i].max(MASS_FLOOR);
                self.wl_recon[1][i] = self.wl_recon[1][i].max(MASS_FLOOR);
                self.wr_recon[1][i] = self.wr_recon[1][i].max(MASS_FLOOR);
                self.wl_recon[4][i] = self.wl_recon[4][i].max(P_MIN);
                self.wr_recon[4][i] = self.wr_recon[4][i].max(P_MIN);
            }
        }
        // --- interior faces (AUSMV) ---
        for f in 1..n {
            let (il, ir) = (f - 1, f);
            let c = 0.5 * (pr.am[il] + pr.am[ir]);
            let (mgl, mll, vgl, vll, pl) = (
                self.wl_recon[0][il],
                self.wl_recon[1][il],
                self.wl_recon[2][il],
                self.wl_recon[3][il],
                self.wl_recon[4][il],
            );
            let (mgr, mlr, vgr, vlr, prr) = (
                self.wr_recon[0][ir],
                self.wr_recon[1][ir],
                self.wr_recon[2][ir],
                self.wr_recon[3][ir],
                self.wr_recon[4][ir],
            );
            let fg = mgl * psi_plus(vgl, c) + mgr * psi_minus(vgr, c);
            let fl = mll * psi_plus(vll, c) + mlr * psi_minus(vlr, c);
            let vml = (mgl * vgl + mll * vll) / (mgl + mll);
            let vmr = (mgr * vgr + mlr * vlr) / (mgr + mlr);
            let pf = p_plus(vml, c) * pl + p_minus(vmr, c) * prr;
            let fm = mgl * vgl * psi_plus(vgl, c)
                + mgr * vgr * psi_minus(vgr, c)
                + mll * vll * psi_plus(vll, c)
                + mlr * vlr * psi_minus(vlr, c)
                + pf;
            self.flux[0][f] = fg;
            self.flux[1][f] = fl;
            self.flux[2][f] = fm;
        }
        // --- inlet: prescribed mass rates; wall when both are zero ---
        {
            let gl = self.wl_in / self.area_face[0];
            let p_in = self.p_anchor.unwrap_or(pr.p[0]);
            let (gg, vl_in) = match (self.makeup_alpha, self.p_anchor) {
                (Some(a_in), Some(pa)) => {
                    // gas make-up feed: reservoir-density gas enters at the
                    // rate the first cell pulls it in; liquid enters at its
                    // own feed velocity, not the (possibly disturbed) cell value
                    let gg = a_in * rho_gas(pa) * pr.vg[0].max(0.0);
                    let vl_in = gl / ((1.0 - a_in) * rho_liq(pa));
                    (gg, vl_in)
                }
                _ => (self.wg_in / self.area_face[0], pr.vl[0]),
            };
            self.flux[0][0] = gg;
            self.flux[1][0] = gl;
            self.flux[2][0] = gg * pr.vg[0].max(0.0) + gl * vl_in + p_in;
        }
        // --- outlet: choke valve to a fixed reservoir pressure ---
        {
            let i = n - 1;
            if self.choke <= 1.0e-4 {
                self.flux[0][n] = 0.0;
                self.flux[1][n] = 0.0;
                self.flux[2][n] = pr.p[i];
            } else {
                let c = pr.am[i];
                // backflow admission: if the last cell pulls inward (v < 0),
                // reservoir fluid at p_out enters through the choke (same
                // void fraction as the cell — zeroth-order composition).
                // Without this the top cell can drain to vacuum after an
                // unloading event: p floors, velocities blow up, dt collapses.
                // Gated smoothly on actual reversal (zero for v >= 0, full by
                // v = -0.1c) so ordinary subsonic outflow — and every
                // validated acceptance result — is untouched.
                let (mg_res, ml_res) = (
                    pr.alpha[i] * rho_gas(self.p_out),
                    (1.0 - pr.alpha[i]) * rho_liq(self.p_out),
                );
                let adm_g = (-pr.vg[i] / (0.1 * c)).clamp(0.0, 1.0);
                let adm_l = (-pr.vl[i] / (0.1 * c)).clamp(0.0, 1.0);
                let fg = mg[i] * psi_plus(pr.vg[i], c) + adm_g * mg_res * psi_minus(pr.vg[i], c);
                let fl = ml[i] * psi_plus(pr.vl[i], c) + adm_l * ml_res * psi_minus(pr.vl[i], c);
                let gtot = fg + fl;
                let rho_m = mg[i] + ml[i];
                let open = self.cv * self.choke;
                let dp = gtot * gtot.abs() / (2.0 * rho_m * open * open);
                self.flux[0][n] = fg;
                self.flux[1][n] = fl;
                self.flux[2][n] = fg * pr.vg[i] + fl * pr.vl[i] + self.p_out + dp;
            }
        }
        // --- divergence + sources ---
        let d = if first_stage {
            &mut self.d1
        } else {
            &mut self.d2
        };
        for i in 0..n {
            let inv = 1.0 / (self.area[i] * self.dx[i]);
            let af0 = self.area_face[i];
            let af1 = self.area_face[i + 1];
            d[0][i] = -(af1 * self.flux[0][i + 1] - af0 * self.flux[0][i]) * inv;
            d[1][i] = -(af1 * self.flux[1][i + 1] - af0 * self.flux[1][i]) * inv;
            let rho_m = mg[i] + ml[i];
            let mut s = -rho_m * self.opts.g * self.sin_th[i];
            if self.opts.wall_friction {
                let a = pr.alpha[i];
                let mu_m = a * MU_G + (1.0 - a) * MU_L;
                let vm = mom[i] / rho_m;
                s += wall_friction(rho_m, vm, self.diam[i], mu_m);
            }
            s += pr.p[i] * (af1 - af0) * inv; // area-change pressure force
            d[2][i] = -(af1 * self.flux[2][i + 1] - af0 * self.flux[2][i]) * inv + s;
        }
        Ok(())
    }
}

fn rho_mix(alpha: f64, p: f64) -> f64 {
    alpha * rho_gas(p) + (1.0 - alpha) * rho_liq(p)
}

fn mc_limiter(a: f64, b: f64) -> f64 {
    if a * b <= 0.0 {
        0.0
    } else {
        let s = if a > 0.0 { 1.0 } else { -1.0 };
        s * (2.0 * a.abs()).min(2.0 * b.abs()).min(0.5 * (a + b).abs())
    }
}

/// Recover primitives from conserved state; NaN/Inf anywhere is an error
/// naming the cell — the sim stops rather than render garbage.
///
/// With regime_feedback on, cells whose (lagged) regime is stratified use a
/// reduced C0: gas in a stratified layer is driven by its own pressure
/// gradient against interfacial shear, not carried with the mixture, so it
/// lags. This is the mechanism that lets gas accumulate in a downhill line —
/// the buildup phase of severe slugging. Blended in alpha to avoid a hard
/// switch at the regime boundary.
fn compute_prim(
    mg: &[f64],
    ml: &[f64],
    mom: &[f64],
    sin_th: &[f64],
    regime: &[u8],
    opts: &Options,
    out: &mut Prim,
) -> Result<(), SimError> {
    const C0_STRAT: f64 = 0.0;
    for i in 0..mg.len() {
        let p = pressure_from_masses(mg[i], ml[i]);
        let rg = rho_gas(p);
        let rl = rho_liq(p);
        let a = (mg[i] / rg).clamp(ALPHA_EPS, 1.0 - ALPHA_EPS);
        let mut c0v = c0(a);
        if opts.regime_feedback && regime[i] <= 1 {
            // keep the single-phase limits exact: blend back to c0(a) at ends
            let w = (4.0 * a * (1.0 - a)).min(1.0);
            c0v = c0v + (C0_STRAT - c0v) * w;
        }
        let vd = drift_velocity(a, rg, rl, sin_th[i], opts.g);
        let (vg, vl) = phase_velocities(mg[i], ml[i], mom[i], a, c0v, vd);
        let am = wood_sound_speed(a, p);
        if !(p.is_finite() && vg.is_finite() && vl.is_finite() && am.is_finite() && am > 0.0) {
            return Err(SimError::NanAtCell(i));
        }
        // unphysical-velocity guard: catches vacuum-type blowups (finite but
        // absurd v from floored mass + finite momentum) as a clean stop
        // instead of a 200k-substep dt-collapse hang
        if vg.abs() > V_MAX || vl.abs() > V_MAX {
            return Err(SimError::BlowupAtCell(i));
        }
        out.p[i] = p;
        out.alpha[i] = a;
        out.rho_g[i] = rg;
        out.rho_l[i] = rl;
        out.vg[i] = vg;
        out.vl[i] = vl;
        out.am[i] = am;
    }
    Ok(())
}
