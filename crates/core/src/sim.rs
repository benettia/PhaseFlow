//! Finite-volume drift-flux simulator: SoA state, AUSMV fluxes, RK2 (Heun),
//! CFL-adaptive or fixed dt, NaN guard that names the offending cell.
//!
//! Conserved fields per cell: `[m_g, m_l, I, e]` — the two phase masses, the
//! mixture momentum, and (only when `Options::thermal` is on) the mixture
//! *internal* energy per unit pipe volume.
//!
//! Why internal and not total energy: temperature must be recoverable before
//! the velocities are, because density depends on T and the velocity solve
//! depends on density. With internal energy, `T = T_ref + e / sum(m_k c_v,k)`
//! is explicit and the pressure inversion stays a closed-form quadratic. A
//! total-energy formulation would need kinetic energy — hence velocities —
//! hence a fixed-point iteration whose count would depend on convergence,
//! which the determinism invariant forbids. The price is that the pressure
//! work `p div(j)` appears as a source rather than inside the flux, so the
//! thermal field is not shock-capturing to machine accuracy. For the
//! processes that set pipeline temperature — ambient heat exchange,
//! blowdown expansion cooling, frictional heating — that price is nil.

use crate::closures::*;
use crate::eos::*;
use crate::fluid::{Fluid, T_MAX, T_MIN};
use crate::regime::{classify, FlowPoint};
use crate::report::{erosional_velocity, Report};
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
/// Conserved fields carried in a snapshot, plus the lagged regime array.
const SNAP_FIELDS: usize = 5;

/// Primitive fields derived from conserved state.
#[derive(Clone, Default)]
struct Prim {
    p: Vec<f64>,
    t: Vec<f64>,
    alpha: Vec<f64>,
    rho_g: Vec<f64>,
    rho_l: Vec<f64>,
    vg: Vec<f64>,
    vl: Vec<f64>,
    am: Vec<f64>, // Wood mixture sound speed
    hg: Vec<f64>, // gas enthalpy flux density m_g*u_g + alpha_g*p [J/m3]
    hl: Vec<f64>, // liquid ditto
}

impl Prim {
    fn with_len(n: usize) -> Self {
        Self {
            p: vec![0.0; n],
            t: vec![0.0; n],
            alpha: vec![0.0; n],
            rho_g: vec![0.0; n],
            rho_l: vec![0.0; n],
            vg: vec![0.0; n],
            vl: vec![0.0; n],
            am: vec![0.0; n],
            hg: vec![0.0; n],
            hl: vec![0.0; n],
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
    rough: Vec<f64>,
    u_wall: Vec<f64>,
    // conserved state
    mg: Vec<f64>,
    ml: Vec<f64>,
    mom: Vec<f64>,
    energy: Vec<f64>,
    prim: Prim,
    regime: Vec<u8>,
    // boundary conditions (live-settable)
    pub wg_in: f64,
    pub wl_in: f64,
    pub p_anchor: Option<f64>,
    pub makeup_alpha: Option<f64>,
    pub t_in: f64,
    pub p_out: f64,
    pub choke: f64,
    cv: f64,
    pub opts: Options,
    pub fluid: Fluid,
    t_amb: f64,
    time: f64,
    dt_last: f64,
    steps: u64,
    /// Boundary mass fluxes recorded by the last first-stage rhs
    /// `[gas_in, liq_in, gas_out, liq_out]` [kg/s].
    bc_w: [f64; 4],
    // scratch
    s_mg: Vec<f64>,
    s_ml: Vec<f64>,
    s_mom: Vec<f64>,
    s_energy: Vec<f64>,
    s_prim: Prim,
    d1: [Vec<f64>; 4],
    d2: [Vec<f64>; 4],
    flux: [Vec<f64>; 4], // n+1 faces
    wl_recon: [Vec<f64>; 7],
    wr_recon: [Vec<f64>; 7],
}

/// Reconstruction slots, named so the flux code reads as physics.
const W_MG: usize = 0;
const W_ML: usize = 1;
const W_VG: usize = 2;
const W_VL: usize = 3;
const W_P: usize = 4;
const W_HG: usize = 5;
const W_HL: usize = 6;

impl Sim {
    pub fn new(sc: &Scenario) -> Result<Self, SimError> {
        let n: usize = sc.segments.iter().map(|s| s.cells).sum();
        assert!((4..=4096).contains(&n), "cell count out of range");
        let f = sc.fluid;
        let t_amb = sc.options.t_ambient.unwrap_or(f.t_ref);
        let mut dx = Vec::with_capacity(n);
        let mut diam = Vec::with_capacity(n);
        let mut sin_th = Vec::with_capacity(n);
        let mut cos_th = Vec::with_capacity(n);
        let mut rough = Vec::with_capacity(n);
        let mut u_wall = Vec::with_capacity(n);
        let mut init = Vec::with_capacity(n);
        for seg in &sc.segments {
            let th = seg.angle_deg.to_radians();
            for _ in 0..seg.cells {
                dx.push(seg.length / seg.cells as f64);
                diam.push(seg.diameter);
                sin_th.push(th.sin());
                cos_th.push(th.cos());
                rough.push(seg.roughness.unwrap_or(sc.options.roughness).max(0.0));
                u_wall.push(seg.u_wall.unwrap_or(sc.options.u_wall).max(0.0));
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
        let temp: Vec<f64> = init
            .iter()
            .map(|s| s.t.unwrap_or(f.t_ref).clamp(T_MIN, T_MAX))
            .collect();
        // initial pressure: as given, optionally replaced by a hydrostatic
        // sweep from the outlet (two fixed-point passes for rho(p))
        let mut p: Vec<f64> = init.iter().map(|s| s.p).collect();
        if sc.options.hydrostatic_init {
            for _ in 0..2 {
                let a_out = init[n - 1].alpha_g;
                p[n - 1] = sc.outlet.p
                    + rho_mix(a_out, p[n - 1], temp[n - 1], &f)
                        * sc.options.g
                        * 0.5
                        * dx[n - 1]
                        * sin_th[n - 1];
                for i in (0..n - 1).rev() {
                    let rm = 0.5
                        * (rho_mix(init[i].alpha_g, p[i], temp[i], &f)
                            + rho_mix(init[i + 1].alpha_g, p[i + 1], temp[i + 1], &f));
                    p[i] = p[i + 1] + rm * sc.options.g * (elev[i + 1] - elev[i]);
                }
            }
        }
        let mut mg = vec![0.0; n];
        let mut ml = vec![0.0; n];
        let mut mom = vec![0.0; n];
        let mut energy = vec![0.0; n];
        for i in 0..n {
            let a = init[i].alpha_g.clamp(ALPHA_EPS, 1.0 - ALPHA_EPS);
            mg[i] = a * f.rho_gas(p[i], temp[i]);
            ml[i] = (1.0 - a) * f.rho_liq(p[i], temp[i]);
            mom[i] = (mg[i] + ml[i]) * init[i].v;
            if sc.options.thermal {
                energy[i] = (mg[i] * f.gas_cv() + ml[i] * f.liq_cp) * (temp[i] - f.t_ref);
            }
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
            rough,
            u_wall,
            mg,
            ml,
            mom,
            energy,
            prim: Prim::with_len(n),
            regime: vec![0; n],
            wg_in: sc.inlet.wg,
            wl_in: sc.inlet.wl,
            p_anchor: sc.inlet.p_anchor,
            makeup_alpha: sc.inlet.makeup_alpha,
            t_in: sc.inlet.t.unwrap_or(f.t_ref).clamp(T_MIN, T_MAX),
            p_out: sc.outlet.p,
            choke: sc.outlet.choke,
            cv: if sc.outlet.cv > 0.0 {
                sc.outlet.cv
            } else {
                0.5
            },
            opts: sc.options,
            fluid: f,
            t_amb,
            time: 0.0,
            dt_last: 0.0,
            steps: 0,
            bc_w: [0.0; 4],
            s_mg: vec![0.0; n],
            s_ml: vec![0.0; n],
            s_mom: vec![0.0; n],
            s_energy: vec![0.0; n],
            s_prim: Prim::with_len(n),
            d1: std::array::from_fn(|_| vec![0.0; n]),
            d2: std::array::from_fn(|_| vec![0.0; n]),
            flux: std::array::from_fn(|_| vec![0.0; n + 1]),
            wl_recon: std::array::from_fn(|_| vec![0.0; n]),
            wr_recon: std::array::from_fn(|_| vec![0.0; n]),
        };
        // bootstrap: recover primitives with neutral regimes (no stratified
        // feedback), classify, then recover again with the real regimes
        sim.regime.fill(2);
        sim.recover(true)?;
        sim.update_regime();
        sim.recover(true)?;
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
    pub fn temperature(&self) -> &[f64] {
        &self.prim.t
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
    pub fn rho_g(&self) -> &[f64] {
        &self.prim.rho_g
    }
    pub fn rho_l(&self) -> &[f64] {
        &self.prim.rho_l
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
    pub fn roughness(&self) -> &[f64] {
        &self.rough
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

    /// Feed temperature [K] and ambient temperature [K], live-settable.
    pub fn set_temperatures(&mut self, t_in: f64, t_ambient: f64) {
        self.t_in = t_in.clamp(T_MIN, T_MAX);
        self.t_amb = t_ambient.clamp(T_MIN, T_MAX);
    }

    pub fn t_ambient(&self) -> f64 {
        self.t_amb
    }

    /// Flat snapshot of everything that evolves: `[mg | ml | mom | e | regime]`,
    /// length 5n. The regime array is carried because with `regime_feedback`
    /// on it is lagged state, not a pure function of the masses — omitting it
    /// would make a restored run diverge from the original.
    pub fn save_state(&self) -> Vec<f64> {
        let mut v = Vec::with_capacity(SNAP_FIELDS * self.n);
        v.extend_from_slice(&self.mg);
        v.extend_from_slice(&self.ml);
        v.extend_from_slice(&self.mom);
        v.extend_from_slice(&self.energy);
        v.extend(self.regime.iter().map(|&r| r as f64));
        v
    }

    pub fn snapshot_len(&self) -> usize {
        SNAP_FIELDS * self.n
    }

    /// Restore a snapshot taken from a `Sim` with identical geometry.
    /// Rollback is exact: the solver is a pure function of (state, steps),
    /// so resuming from a restored snapshot reproduces the trajectory the
    /// original run would have taken (`steps` matters because the regime
    /// refresh runs on a fixed step cadence).
    pub fn load_state(&mut self, data: &[f64], time: f64, steps: u64) -> Result<(), SimError> {
        assert_eq!(data.len(), SNAP_FIELDS * self.n, "snapshot length mismatch");
        let n = self.n;
        self.mg.copy_from_slice(&data[..n]);
        self.ml.copy_from_slice(&data[n..2 * n]);
        self.mom.copy_from_slice(&data[2 * n..3 * n]);
        self.energy.copy_from_slice(&data[3 * n..4 * n]);
        for i in 0..n {
            self.regime[i] = data[4 * n + i] as u8;
        }
        self.time = time;
        self.steps = steps;
        self.dt_last = 0.0;
        self.recover(true)
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

    /// Primitive recovery into `prim` (main state) or `s_prim` (stage state).
    fn recover(&mut self, main: bool) -> Result<(), SimError> {
        let (mg, ml, mom, e, out) = if main {
            (&self.mg, &self.ml, &self.mom, &self.energy, &mut self.prim)
        } else {
            (
                &self.s_mg,
                &self.s_ml,
                &self.s_mom,
                &self.s_energy,
                &mut self.s_prim,
            )
        };
        compute_prim(
            mg,
            ml,
            mom,
            e,
            &self.sin_th,
            &self.regime,
            &self.opts,
            &self.fluid,
            out,
        )
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
            self.s_energy[i] = self.energy[i] + dt * self.d1[3][i];
        }
        self.recover(false)?;
        // stage 2: U^{n+1} = U + dt/2 (L(U) + L(U1))
        self.rhs(false)?; // fills d2 using s_* state
        for i in 0..self.n {
            self.mg[i] = (self.mg[i] + 0.5 * dt * (self.d1[0][i] + self.d2[0][i])).max(MASS_FLOOR);
            self.ml[i] = (self.ml[i] + 0.5 * dt * (self.d1[1][i] + self.d2[1][i])).max(MASS_FLOOR);
            self.mom[i] += 0.5 * dt * (self.d1[2][i] + self.d2[2][i]);
            self.energy[i] += 0.5 * dt * (self.d1[3][i] + self.d2[3][i]);
        }
        self.recover(true)?;
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
            let t = self.prim.t[i];
            self.regime[i] = classify(&FlowPoint {
                alpha: a,
                jg: a * self.prim.vg[i],
                jl: (1.0 - a) * self.prim.vl[i],
                d: self.diam[i],
                sin_th: self.sin_th[i],
                cos_th: self.cos_th[i],
                rho_g: self.prim.rho_g[i],
                rho_l: self.prim.rho_l[i],
                mu_g: self.fluid.mu_gas(t),
                mu_l: self.fluid.mu_liq(t),
                g: self.opts.g,
            }) as u8;
        }
    }

    /// Design quantities for the current state — pressure-drop split,
    /// inventory, erosional-velocity check. See `crate::report`.
    pub fn report(&self) -> Report {
        let pr = &self.prim;
        let n = self.n;
        let mut r = Report {
            p_min: f64::INFINITY,
            p_max: f64::NEG_INFINITY,
            t_min: f64::INFINITY,
            t_max: f64::NEG_INFINITY,
            a_min: f64::INFINITY,
            ..Default::default()
        };
        let mut vol = 0.0;
        for i in 0..n {
            let cell_vol = self.area[i] * self.dx[i];
            let rho_m = self.mg[i] + self.ml[i];
            vol += cell_vol;
            r.gas_inventory += self.mg[i] * cell_vol;
            r.liquid_inventory += self.ml[i] * cell_vol;
            r.holdup_avg += (1.0 - pr.alpha[i]) * cell_vol;

            // gravity term: +rho g sin(theta) dx is the pressure lost climbing
            r.dp_gravity += rho_m * self.opts.g * self.sin_th[i] * self.dx[i];
            if self.opts.wall_friction {
                let a = pr.alpha[i];
                let t = pr.t[i];
                let mu_m = a * self.fluid.mu_gas(t) + (1.0 - a) * self.fluid.mu_liq(t);
                let vm = self.mom[i] / rho_m;
                // wall_friction returns the momentum source [Pa/m]; the loss
                // along the flow is its negative
                r.dp_friction -=
                    wall_friction(rho_m, vm, self.diam[i], mu_m, self.rough[i]) * self.dx[i];
            }

            let vm = self.mom[i] / rho_m;
            if vm.abs() > r.v_mix_max {
                r.v_mix_max = vm.abs();
                r.v_mix_max_cell = i;
            }
            let ratio = vm.abs() / erosional_velocity(rho_m);
            if ratio > r.erosion_ratio {
                r.erosion_ratio = ratio;
                r.erosion_cell = i;
            }
            r.p_min = r.p_min.min(pr.p[i]);
            r.p_max = r.p_max.max(pr.p[i]);
            r.t_min = r.t_min.min(pr.t[i]);
            r.t_max = r.t_max.max(pr.t[i]);
            r.a_min = r.a_min.min(pr.am[i]);
        }
        r.holdup_avg /= vol;
        r.dp_total = pr.p[0] - pr.p[n - 1];
        r.dp_accel = r.dp_total - r.dp_friction - r.dp_gravity;
        [r.w_gas_in, r.w_liq_in, r.w_gas_out, r.w_liq_out] = self.bc_w;
        r
    }

    /// Spatial operator L(U) -> d[0..4]. `first_stage` selects state buffers.
    fn rhs(&mut self, first_stage: bool) -> Result<(), SimError> {
        let n = self.n;
        let thermal = self.opts.thermal;
        let (mg, ml, mom, pr) = if first_stage {
            (&self.mg, &self.ml, &self.mom, &self.prim)
        } else {
            (&self.s_mg, &self.s_ml, &self.s_mom, &self.s_prim)
        };
        // --- reconstruction: W = [mg, ml, vg, vl, p, h_g, h_l] ---
        {
            let vars: [&[f64]; 7] = [mg, ml, &pr.vg, &pr.vl, &pr.p, &pr.hg, &pr.hl];
            let used = if thermal { 7 } else { 5 };
            for (k, w) in vars.iter().enumerate().take(used) {
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
                self.wl_recon[W_MG][i] = self.wl_recon[W_MG][i].max(MASS_FLOOR);
                self.wr_recon[W_MG][i] = self.wr_recon[W_MG][i].max(MASS_FLOOR);
                self.wl_recon[W_ML][i] = self.wl_recon[W_ML][i].max(MASS_FLOOR);
                self.wr_recon[W_ML][i] = self.wr_recon[W_ML][i].max(MASS_FLOOR);
                self.wl_recon[W_P][i] = self.wl_recon[W_P][i].max(P_MIN);
                self.wr_recon[W_P][i] = self.wr_recon[W_P][i].max(P_MIN);
            }
        }
        // --- interior faces (AUSMV) ---
        for f in 1..n {
            let (il, ir) = (f - 1, f);
            let c = 0.5 * (pr.am[il] + pr.am[ir]);
            let (mgl, mll, vgl, vll, pl) = (
                self.wl_recon[W_MG][il],
                self.wl_recon[W_ML][il],
                self.wl_recon[W_VG][il],
                self.wl_recon[W_VL][il],
                self.wl_recon[W_P][il],
            );
            let (mgr, mlr, vgr, vlr, prr) = (
                self.wr_recon[W_MG][ir],
                self.wr_recon[W_ML][ir],
                self.wr_recon[W_VG][ir],
                self.wr_recon[W_VL][ir],
                self.wr_recon[W_P][ir],
            );
            let (pg_p, pg_m) = (psi_plus(vgl, c), psi_minus(vgr, c));
            let (pl_p, pl_m) = (psi_plus(vll, c), psi_minus(vlr, c));
            let fg = mgl * pg_p + mgr * pg_m;
            let fl = mll * pl_p + mlr * pl_m;
            let vml = (mgl * vgl + mll * vll) / (mgl + mll);
            let vmr = (mgr * vgr + mlr * vlr) / (mgr + mlr);
            let pf = p_plus(vml, c) * pl + p_minus(vmr, c) * prr;
            let fm = mgl * vgl * pg_p + mgr * vgr * pg_m + mll * vll * pl_p + mlr * vlr * pl_m + pf;
            self.flux[0][f] = fg;
            self.flux[1][f] = fl;
            self.flux[2][f] = fm;
            self.flux[3][f] = if thermal {
                self.wl_recon[W_HG][il] * pg_p
                    + self.wr_recon[W_HG][ir] * pg_m
                    + self.wl_recon[W_HL][il] * pl_p
                    + self.wr_recon[W_HL][ir] * pl_m
            } else {
                0.0
            };
        }
        // --- inlet: prescribed mass rates; wall when both are zero ---
        {
            let f = &self.fluid;
            let gl = self.wl_in / self.area_face[0];
            let p_in = self.p_anchor.unwrap_or(pr.p[0]);
            let (gg, vl_in) = match (self.makeup_alpha, self.p_anchor) {
                (Some(a_in), Some(pa)) => {
                    // gas make-up feed: reservoir-density gas enters at the
                    // rate the first cell pulls it in; liquid enters at its
                    // own feed velocity, not the (possibly disturbed) cell value
                    let gg = a_in * f.rho_gas(pa, self.t_in) * pr.vg[0].max(0.0);
                    let vl_in = gl / ((1.0 - a_in) * f.rho_liq(pa, self.t_in));
                    (gg, vl_in)
                }
                _ => (self.wg_in / self.area_face[0], pr.vl[0]),
            };
            self.flux[0][0] = gg;
            self.flux[1][0] = gl;
            if first_stage {
                self.bc_w[0] = gg * self.area_face[0];
                self.bc_w[1] = gl * self.area_face[0];
            }
            self.flux[2][0] = gg * pr.vg[0].max(0.0) + gl * vl_in + p_in;
            // feed enthalpy: h = u(T_in) + p/rho at the feed state
            self.flux[3][0] = if thermal {
                let dt_in = self.t_in - f.t_ref;
                let hg = f.gas_cv() * dt_in + p_in / f.rho_gas(p_in, self.t_in);
                let hl = f.liq_cp * dt_in + p_in / f.rho_liq(p_in, self.t_in);
                gg * hg + gl * hl
            } else {
                0.0
            };
        }
        // --- outlet: choke valve to a fixed reservoir pressure ---
        {
            let i = n - 1;
            let f = &self.fluid;
            if self.choke <= 1.0e-4 {
                self.flux[0][n] = 0.0;
                self.flux[1][n] = 0.0;
                self.flux[2][n] = pr.p[i];
                self.flux[3][n] = 0.0;
                if first_stage {
                    self.bc_w[2] = 0.0;
                    self.bc_w[3] = 0.0;
                }
            } else {
                let c = pr.am[i];
                // AUSMV needs an exterior state at this face. Outflow sees the
                // interior state extrapolated (so psi+ and psi- recombine to
                // exactly m*v — dropping the psi- half over-discharges by
                // ~c/4 per unit mass, which the last cell used to absorb by
                // drawing in gas until its own Wood speed fell far enough to
                // balance: that was the "outlet boundary layer" void blip).
                // Reversal sees the reservoir instead, so fluid can come back
                // in through the choke rather than draining the cell to
                // vacuum. The blend is smooth: interior for v >= 0, reservoir
                // by v = -0.1c.
                let (rg_res, rl_res) = (
                    f.rho_gas(self.p_out, self.t_amb),
                    f.rho_liq(self.p_out, self.t_amb),
                );
                let (mg_res, ml_res) = (pr.alpha[i] * rg_res, (1.0 - pr.alpha[i]) * rl_res);
                let adm_g = (-pr.vg[i] / (0.1 * c)).clamp(0.0, 1.0);
                let adm_l = (-pr.vl[i] / (0.1 * c)).clamp(0.0, 1.0);
                let mg_ext = mg[i] + adm_g * (mg_res - mg[i]);
                let ml_ext = ml[i] + adm_l * (ml_res - ml[i]);
                let (pg_p, pg_m) = (psi_plus(pr.vg[i], c), psi_minus(pr.vg[i], c));
                let (pl_p, pl_m) = (psi_plus(pr.vl[i], c), psi_minus(pr.vl[i], c));
                let fg = mg[i] * pg_p + mg_ext * pg_m;
                let fl = ml[i] * pl_p + ml_ext * pl_m;
                let gtot = fg + fl;
                let rho_m = mg[i] + ml[i];
                let open = self.cv * self.choke;
                let dp = gtot * gtot.abs() / (2.0 * rho_m * open * open);
                self.flux[0][n] = fg;
                self.flux[1][n] = fl;
                self.flux[2][n] = fg * pr.vg[i] + fl * pr.vl[i] + self.p_out + dp;
                if first_stage {
                    self.bc_w[2] = fg * self.area_face[n];
                    self.bc_w[3] = fl * self.area_face[n];
                }
                self.flux[3][n] = if thermal {
                    let dt_res = self.t_amb - f.t_ref;
                    let hg_res = mg_res * f.gas_cv() * dt_res + pr.alpha[i] * self.p_out;
                    let hl_res = ml_res * f.liq_cp * dt_res + (1.0 - pr.alpha[i]) * self.p_out;
                    let hg_ext = pr.hg[i] + adm_g * (hg_res - pr.hg[i]);
                    let hl_ext = pr.hl[i] + adm_l * (hl_res - pr.hl[i]);
                    pr.hg[i] * pg_p + hg_ext * pg_m + pr.hl[i] * pl_p + hl_ext * pl_m
                } else {
                    0.0
                };
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
            let mut fric = 0.0;
            let vm = mom[i] / rho_m;
            if self.opts.wall_friction {
                let a = pr.alpha[i];
                let t = pr.t[i];
                let mu_m = a * self.fluid.mu_gas(t) + (1.0 - a) * self.fluid.mu_liq(t);
                fric = wall_friction(rho_m, vm, self.diam[i], mu_m, self.rough[i]);
                s += fric;
            }
            s += pr.p[i] * (af1 - af0) * inv; // area-change pressure force
            d[2][i] = -(af1 * self.flux[2][i + 1] - af0 * self.flux[2][i]) * inv + s;
            d[3][i] = if thermal {
                // j dp/dx (reversible compression work; the p div(j) half is
                // already inside the enthalpy flux), frictional dissipation,
                // and wall heat exchange.
                let a = pr.alpha[i];
                let j = a * pr.vg[i] + (1.0 - a) * pr.vl[i];
                let lo = i.saturating_sub(1);
                let hi = (i + 1).min(n - 1);
                let dpdx = if hi > lo {
                    (pr.p[hi] - pr.p[lo]) / (self.x_mid[hi] - self.x_mid[lo])
                } else {
                    0.0
                };
                let q_wall = 4.0 * self.u_wall[i] / self.diam[i] * (self.t_amb - pr.t[i]);
                -(af1 * self.flux[3][i + 1] - af0 * self.flux[3][i]) * inv + j * dpdx - fric * vm
                    + q_wall
            } else {
                0.0
            };
        }
        Ok(())
    }
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
#[allow(clippy::too_many_arguments)]
fn compute_prim(
    mg: &[f64],
    ml: &[f64],
    mom: &[f64],
    energy: &[f64],
    sin_th: &[f64],
    regime: &[u8],
    opts: &Options,
    fluid: &Fluid,
    out: &mut Prim,
) -> Result<(), SimError> {
    const C0_STRAT: f64 = 0.0;
    let cvg = fluid.gas_cv();
    let cvl = fluid.liq_cp;
    for i in 0..mg.len() {
        // temperature first: density, and therefore everything else, needs it
        let t = if opts.thermal {
            let cap = mg[i] * cvg + ml[i] * cvl;
            if cap.is_nan() || cap <= 0.0 {
                return Err(SimError::NanAtCell(i));
            }
            fluid.t_ref + energy[i] / cap
        } else {
            fluid.t_ref
        };
        // NaN fails `contains`, so this one test covers the NaN path too
        if !(T_MIN..=T_MAX).contains(&t) {
            return Err(SimError::BlowupAtCell(i));
        }
        let p = pressure_from_masses(mg[i], ml[i], t, fluid);
        let rg = fluid.rho_gas(p, t);
        let rl = fluid.rho_liq(p, t);
        let a = (mg[i] / rg).clamp(ALPHA_EPS, 1.0 - ALPHA_EPS);
        let mut c0v = c0(a);
        if opts.regime_feedback && regime[i] <= 1 {
            // keep the single-phase limits exact: blend back to c0(a) at ends
            let w = (4.0 * a * (1.0 - a)).min(1.0);
            c0v = c0v + (C0_STRAT - c0v) * w;
        }
        let vd = drift_velocity(a, rg, rl, sin_th[i], opts.g, fluid.sigma);
        let (vg, vl) = phase_velocities(mg[i], ml[i], mom[i], a, c0v, vd);
        let am = wood_sound_speed(a, p, t, fluid, opts.thermal);
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
        out.t[i] = t;
        out.alpha[i] = a;
        out.rho_g[i] = rg;
        out.rho_l[i] = rl;
        out.vg[i] = vg;
        out.vl[i] = vl;
        out.am[i] = am;
        if opts.thermal {
            let dt = t - fluid.t_ref;
            out.hg[i] = mg[i] * cvg * dt + a * p;
            out.hl[i] = ml[i] * cvl * dt + (1.0 - a) * p;
        }
    }
    Ok(())
}
