//! wasm-bindgen wrapper. State getters return Float64Array views straight
//! into wasm linear memory (no copies): views are invalidated whenever wasm
//! memory grows, so the JS side re-acquires them every frame (they are cheap).

use phase_core::eos::{rho_gas, rho_liq, wood_sound_speed};
use phase_core::{classify, Sim, SimError};
use wasm_bindgen::prelude::*;

fn err(e: SimError) -> JsError {
    match e {
        SimError::NanAtCell(i) => JsError::new(&format!("NaN detected in cell {i}: sim stopped")),
        SimError::BlowupAtCell(i) => {
            JsError::new(&format!("unphysical velocity in cell {i}: sim stopped"))
        }
        SimError::MaxSubsteps => JsError::new("exceeded max substeps in one advance() call"),
    }
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

    pub fn set_muscl(&mut self, on: bool) {
        self.sim.opts.muscl = on;
    }

    /// Snapshot for the timeline: `[mg | ml | mom | regime]`, length 4n.
    pub fn save_state(&self) -> js_sys::Float64Array {
        js_sys::Float64Array::from(&self.sim.save_state()[..])
    }

    /// Roll back to a snapshot; the continuation is bit-identical to what
    /// the original run would have produced.
    pub fn load_state(&mut self, data: &[f64], time: f64, steps: f64) -> Result<(), JsError> {
        if data.len() != 4 * self.sim.n {
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

    pub fn alpha(&self) -> js_sys::Float64Array {
        view(self.sim.alpha())
    }
    pub fn p(&self) -> js_sys::Float64Array {
        view(self.sim.p())
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
    pub fn x_mid(&self) -> js_sys::Float64Array {
        view(self.sim.x_mid())
    }
    pub fn elev(&self) -> js_sys::Float64Array {
        view(self.sim.elev())
    }
    pub fn diam(&self) -> js_sys::Float64Array {
        view(self.sim.diam())
    }
    pub fn regime(&self) -> js_sys::Uint8Array {
        js_sys::Uint8Array::from(self.sim.regime())
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

/// Classify a (jg, jl) point for the Taitel-Dukler map background.
#[wasm_bindgen]
pub fn classify_point(jg: f64, jl: f64, d: f64, sin_th: f64, cos_th: f64, p: f64) -> u8 {
    let a = (jg / (jg + jl).max(1e-9)).clamp(1e-4, 1.0 - 1e-4);
    classify(a, jg, jl, d, sin_th, cos_th, rho_gas(p), rho_liq(p), 9.81) as u8
}

/// Wood mixture sound speed, exposed for the UI.
#[wasm_bindgen]
pub fn wood_speed(alpha: f64, p: f64) -> f64 {
    wood_sound_speed(alpha, p)
}
