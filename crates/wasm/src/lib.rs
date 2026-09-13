//! wasm-bindgen wrapper. State getters return Float64Array views straight
//! into wasm linear memory (no copies): views are invalidated whenever wasm
//! memory grows, so the JS side re-acquires them every frame (they are cheap).
//!
//! Anything fluid-dependent (regime map, sound speed) is a *method* on the
//! sim rather than a free function, so the panel can never draw a map for a
//! different fluid than the one being solved.

use phase_core::eos::wood_sound_speed;
use phase_core::{classify, FlowPoint, Sim, SimError};
use wasm_bindgen::prelude::*;

fn err(e: SimError) -> JsError {
    match e {
        SimError::NanAtCell(i) => JsError::new(&format!("NaN detected in cell {i}: sim stopped")),
        SimError::BlowupAtCell(i) => JsError::new(&format!(
            "cell {i} left the physical envelope (velocity or temperature): sim stopped"
        )),
        SimError::MaxSubsteps => JsError::new("exceeded max substeps in one advance() call"),
    }
}

fn obj(pairs: &[(&str, f64)]) -> js_sys::Object {
    let o = js_sys::Object::new();
    for (k, v) in pairs {
        let _ = js_sys::Reflect::set(&o, &JsValue::from_str(k), &JsValue::from_f64(*v));
    }
    o
}

#[wasm_bindgen]
pub struct WasmSim {
    sim: Sim,
}

#[wasm_bindgen]
impl WasmSim {
    #[wasm_bindgen(constructor)]
    pub fn new(scenario_json: &str) -> Result<WasmSim, JsError> {
        let sc = phase_scenario::parse(scenario_json).map_err(|e| JsError::new(&e))?;
        Ok(WasmSim {
            sim: Sim::new(&sc).map_err(err)?,
        })
    }

    /// Advance simulated time by dt_ms milliseconds (internal CFL substeps).
    pub fn step(&mut self, dt_ms: f64) -> Result<(), JsError> {
        self.sim.advance(dt_ms / 1000.0).map_err(err)
    }

    /// One solver step; returns the dt taken [s].
    pub fn single_step(&mut self) -> Result<f64, JsError> {
        self.sim.step().map_err(err)
    }

    pub fn set_bc(&mut self, wg: f64, wl: f64, p_out: f64, choke: f64) {
        self.sim.set_bc(wg, wl, p_out, choke);
    }

    /// Feed and ambient temperatures [K].
    pub fn set_temperatures(&mut self, t_in: f64, t_ambient: f64) {
        self.sim.set_temperatures(t_in, t_ambient);
    }

    pub fn set_muscl(&mut self, on: bool) {
        self.sim.opts.muscl = on;
    }

    /// Snapshot for the timeline: `[mg | ml | mom | e | regime]`, length 5n.
    pub fn save_state(&self) -> js_sys::Float64Array {
        js_sys::Float64Array::from(&self.sim.save_state()[..])
    }

    pub fn snapshot_len(&self) -> usize {
        self.sim.snapshot_len()
    }

    /// Roll back to a snapshot; the continuation is bit-identical to what
    /// the original run would have produced.
    pub fn load_state(&mut self, data: &[f64], time: f64, steps: f64) -> Result<(), JsError> {
        if data.len() != self.sim.snapshot_len() {
            return Err(JsError::new("snapshot does not match this geometry"));
        }
        self.sim.load_state(data, time, steps as u64).map_err(err)
    }

    pub fn steps(&self) -> f64 {
        self.sim.steps() as f64
    }

    pub fn refresh_regime(&mut self) {
        self.sim.update_regime();
    }

    pub fn time(&self) -> f64 {
        self.sim.time()
    }
    pub fn dt_last(&self) -> f64 {
        self.sim.dt_last()
    }
    pub fn n_cells(&self) -> usize {
        self.sim.n
    }
    pub fn thermal(&self) -> bool {
        self.sim.opts.thermal
    }

    pub fn alpha(&self) -> js_sys::Float64Array {
        view(self.sim.alpha())
    }
    pub fn p(&self) -> js_sys::Float64Array {
        view(self.sim.p())
    }
    pub fn temperature(&self) -> js_sys::Float64Array {
        view(self.sim.temperature())
    }
    pub fn vg(&self) -> js_sys::Float64Array {
        view(self.sim.vg())
    }
    pub fn vl(&self) -> js_sys::Float64Array {
        view(self.sim.vl())
    }
    pub fn am(&self) -> js_sys::Float64Array {
        view(self.sim.am())
    }
    pub fn rho_g(&self) -> js_sys::Float64Array {
        view(self.sim.rho_g())
    }
    pub fn rho_l(&self) -> js_sys::Float64Array {
        view(self.sim.rho_l())
    }
    pub fn x_mid(&self) -> js_sys::Float64Array {
        view(self.sim.x_mid())
    }
    pub fn elev(&self) -> js_sys::Float64Array {
        view(self.sim.elev())
    }
    pub fn diam(&self) -> js_sys::Float64Array {
        view(self.sim.diam())
    }
    pub fn area(&self) -> js_sys::Float64Array {
        view(self.sim.area())
    }
    pub fn dx(&self) -> js_sys::Float64Array {
        view(self.sim.dx())
    }
    pub fn regime(&self) -> js_sys::Uint8Array {
        js_sys::Uint8Array::from(self.sim.regime())
    }

    /// Design quantities for the current instant (see `phase_core::report`).
    pub fn report(&self) -> js_sys::Object {
        let r = self.sim.report();
        obj(&[
            ("dpTotal", r.dp_total),
            ("dpFriction", r.dp_friction),
            ("dpGravity", r.dp_gravity),
            ("dpAccel", r.dp_accel),
            ("gasInventory", r.gas_inventory),
            ("liquidInventory", r.liquid_inventory),
            ("holdupAvg", r.holdup_avg),
            ("wGasIn", r.w_gas_in),
            ("wLiqIn", r.w_liq_in),
            ("wGasOut", r.w_gas_out),
            ("wLiqOut", r.w_liq_out),
            ("vMixMax", r.v_mix_max),
            ("vMixMaxCell", r.v_mix_max_cell as f64),
            ("erosionRatio", r.erosion_ratio),
            ("erosionCell", r.erosion_cell as f64),
            ("pMin", r.p_min),
            ("pMax", r.p_max),
            ("tMin", r.t_min),
            ("tMax", r.t_max),
            ("aMin", r.a_min),
        ])
    }

    /// The resolved fluid properties, for the panel's property readout.
    pub fn fluid(&self) -> js_sys::Object {
        let f = &self.sim.fluid;
        obj(&[
            ("gasMw", f.gas_mw),
            ("gasZ", f.gas_z),
            ("gasMu", f.gas_mu),
            ("gasCp", f.gas_cp),
            ("liqRho", f.liq_rho),
            ("liqA", f.liq_a),
            ("liqBeta", f.liq_beta),
            ("liqMu", f.liq_mu),
            ("liqCp", f.liq_cp),
            ("sigma", f.sigma),
            ("pRef", f.p_ref),
            ("tRef", f.t_ref),
            ("rGas", f.r_gas()),
            ("gamma", f.gamma()),
            ("aGas", f.a_gas2(f.t_ref, false).sqrt()),
            ("tAmbient", self.sim.t_ambient()),
        ])
    }

    /// Classify a (jg, jl) point *with this sim's fluid* — the Taitel-Dukler
    /// map background. `p` and `t` set the property state.
    #[allow(clippy::too_many_arguments)]
    pub fn classify_point(
        &self,
        jg: f64,
        jl: f64,
        d: f64,
        sin_th: f64,
        cos_th: f64,
        p: f64,
        t: f64,
    ) -> u8 {
        let f = &self.sim.fluid;
        let a = (jg / (jg + jl).max(1e-9)).clamp(1e-4, 1.0 - 1e-4);
        classify(&FlowPoint {
            alpha: a,
            jg,
            jl,
            d,
            sin_th,
            cos_th,
            rho_g: f.rho_gas(p, t),
            rho_l: f.rho_liq(p, t),
            mu_g: f.mu_gas(t),
            mu_l: f.mu_liq(t),
            g: self.sim.opts.g,
        }) as u8
    }

    /// Wood mixture sound speed with this sim's fluid [m/s].
    pub fn wood_speed(&self, alpha: f64, p: f64, t: f64) -> f64 {
        wood_sound_speed(alpha, p, t, &self.sim.fluid, self.sim.opts.thermal)
    }

    pub fn min_elev_cell(&self) -> usize {
        let e = self.sim.elev();
        let mut best = 0;
        for i in 1..e.len() {
            if e[i] < e[best] {
                best = i;
            }
        }
        best
    }
}

fn view(s: &[f64]) -> js_sys::Float64Array {
    // SAFETY: zero-copy view into wasm linear memory. Views are invalidated
    // when memory grows, so the JS side re-acquires them every frame and
    // never holds one across a scenario rebuild.
    unsafe { js_sys::Float64Array::view(s) }
}
