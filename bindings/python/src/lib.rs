//! pyo3 bindings: `import phase_flow`. Arrays come out as numpy copies —
//! never aliased mutable views into live solver state (safe by construction;
//! a copy of a few KB per call is nothing next to the step cost).

use numpy::{PyArray1, PyArrayMethods};
use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;
use pyo3::types::PyDict;

use phase_core::eos::wood_sound_speed;
use phase_core::fluid::Fluid;
use phase_core::{classify, FlowPoint, SimError};

fn fluid_or_err(name: &str) -> PyResult<Fluid> {
    Fluid::by_name(name).ok_or_else(|| PyRuntimeError::new_err(format!("unknown fluid '{name}'")))
}

fn err(e: SimError) -> PyErr {
    match e {
        SimError::NanAtCell(i) => PyRuntimeError::new_err(format!("NaN detected in cell {i}")),
        SimError::BlowupAtCell(i) => {
            PyRuntimeError::new_err(format!("unphysical velocity in cell {i}"))
        }
        SimError::MaxSubsteps => PyRuntimeError::new_err("exceeded max substeps"),
    }
}

#[pyclass]
struct Sim {
    inner: phase_core::Sim,
}

fn arr<'py>(py: Python<'py>, s: &[f64]) -> Bound<'py, PyArray1<f64>> {
    PyArray1::from_slice(py, s)
}

#[pymethods]
impl Sim {
    #[staticmethod]
    fn from_json(scenario_json: &str) -> PyResult<Self> {
        let sc = phase_scenario::parse(scenario_json).map_err(PyRuntimeError::new_err)?;
        Ok(Sim {
            inner: phase_core::Sim::new(&sc).map_err(err)?,
        })
    }

    /// Advance simulated time by t seconds (internal CFL substeps).
    fn run(&mut self, t: f64) -> PyResult<()> {
        self.inner.advance(t).map_err(err)
    }

    /// One solver step; returns dt taken [s].
    fn step(&mut self) -> PyResult<f64> {
        self.inner.step().map_err(err)
    }

    #[pyo3(signature = (wg, wl, p_out, choke))]
    fn set_bc(&mut self, wg: f64, wl: f64, p_out: f64, choke: f64) {
        self.inner.set_bc(wg, wl, p_out, choke);
    }

    fn set_muscl(&mut self, on: bool) {
        self.inner.opts.muscl = on;
    }

    /// Feed and ambient temperatures [K].
    fn set_temperatures(&mut self, t_in: f64, t_ambient: f64) {
        self.inner.set_temperatures(t_in, t_ambient);
    }

    fn refresh_regime(&mut self) {
        self.inner.update_regime();
    }

    #[getter]
    fn time(&self) -> f64 {
        self.inner.time()
    }
    #[getter]
    fn dt_last(&self) -> f64 {
        self.inner.dt_last()
    }
    #[getter]
    fn n_cells(&self) -> usize {
        self.inner.n
    }
    #[getter]
    fn alpha_g<'py>(&self, py: Python<'py>) -> Bound<'py, PyArray1<f64>> {
        arr(py, self.inner.alpha())
    }
    #[getter]
    fn p<'py>(&self, py: Python<'py>) -> Bound<'py, PyArray1<f64>> {
        arr(py, self.inner.p())
    }
    #[getter]
    fn vg<'py>(&self, py: Python<'py>) -> Bound<'py, PyArray1<f64>> {
        arr(py, self.inner.vg())
    }
    #[getter]
    fn vl<'py>(&self, py: Python<'py>) -> Bound<'py, PyArray1<f64>> {
        arr(py, self.inner.vl())
    }
    #[getter]
    fn am<'py>(&self, py: Python<'py>) -> Bound<'py, PyArray1<f64>> {
        arr(py, self.inner.am())
    }
    #[getter]
    fn x<'py>(&self, py: Python<'py>) -> Bound<'py, PyArray1<f64>> {
        arr(py, self.inner.x_mid())
    }
    #[getter]
    fn elevation<'py>(&self, py: Python<'py>) -> Bound<'py, PyArray1<f64>> {
        arr(py, self.inner.elev())
    }
    #[getter]
    fn dx<'py>(&self, py: Python<'py>) -> Bound<'py, PyArray1<f64>> {
        arr(py, self.inner.dx())
    }
    #[getter]
    fn regime<'py>(&self, py: Python<'py>) -> Bound<'py, PyArray1<u8>> {
        PyArray1::from_slice(py, self.inner.regime())
    }
    #[getter]
    fn temperature<'py>(&self, py: Python<'py>) -> Bound<'py, PyArray1<f64>> {
        arr(py, self.inner.temperature())
    }
    #[getter]
    fn rho_g<'py>(&self, py: Python<'py>) -> Bound<'py, PyArray1<f64>> {
        arr(py, self.inner.rho_g())
    }
    #[getter]
    fn rho_l<'py>(&self, py: Python<'py>) -> Bound<'py, PyArray1<f64>> {
        arr(py, self.inner.rho_l())
    }
    #[getter]
    fn diameter<'py>(&self, py: Python<'py>) -> Bound<'py, PyArray1<f64>> {
        arr(py, self.inner.diam())
    }
    #[getter]
    fn area<'py>(&self, py: Python<'py>) -> Bound<'py, PyArray1<f64>> {
        arr(py, self.inner.area())
    }

    /// Design quantities for the current instant: pressure-drop split,
    /// inventory, boundary rates, erosional-velocity check.
    fn report<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let r = self.inner.report();
        let d = PyDict::new(py);
        d.set_item("dp_total", r.dp_total)?;
        d.set_item("dp_friction", r.dp_friction)?;
        d.set_item("dp_gravity", r.dp_gravity)?;
        d.set_item("dp_accel", r.dp_accel)?;
        d.set_item("gas_inventory", r.gas_inventory)?;
        d.set_item("liquid_inventory", r.liquid_inventory)?;
        d.set_item("holdup_avg", r.holdup_avg)?;
        d.set_item("w_gas_in", r.w_gas_in)?;
        d.set_item("w_liq_in", r.w_liq_in)?;
        d.set_item("w_gas_out", r.w_gas_out)?;
        d.set_item("w_liq_out", r.w_liq_out)?;
        d.set_item("v_mix_max", r.v_mix_max)?;
        d.set_item("v_mix_max_cell", r.v_mix_max_cell)?;
        d.set_item("erosion_ratio", r.erosion_ratio)?;
        d.set_item("erosion_cell", r.erosion_cell)?;
        d.set_item("p_min", r.p_min)?;
        d.set_item("p_max", r.p_max)?;
        d.set_item("t_min", r.t_min)?;
        d.set_item("t_max", r.t_max)?;
        d.set_item("a_min", r.a_min)?;
        Ok(d)
    }

    /// (gas_mass, liquid_mass) totals [kg].
    fn mass_totals(&self) -> (f64, f64) {
        self.inner.mass_totals()
    }

    /// Checkpoint: flat `[mg | ml | mom | e | regime]` array, length 5n.
    fn save_state<'py>(&self, py: Python<'py>) -> Bound<'py, PyArray1<f64>> {
        PyArray1::from_vec(py, self.inner.save_state())
    }

    /// Restore a checkpoint taken from an identical geometry; the
    /// continuation reproduces the original trajectory exactly.
    fn load_state(
        &mut self,
        data: numpy::PyReadonlyArray1<f64>,
        time: f64,
        steps: u64,
    ) -> PyResult<()> {
        let slice = data
            .as_slice()
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        if slice.len() != self.inner.snapshot_len() {
            return Err(PyRuntimeError::new_err(
                "snapshot does not match this geometry",
            ));
        }
        self.inner.load_state(slice, time, steps).map_err(err)
    }

    #[getter]
    fn steps(&self) -> u64 {
        self.inner.steps()
    }
}

/// Classify a flow point; returns the regime code 0..8.
#[pyfunction]
#[pyo3(signature = (alpha, jg, jl, d, sin_th=0.0, cos_th=1.0, p=1.0e5, g=9.81, fluid="air-water", t=None))]
#[allow(clippy::too_many_arguments)]
fn classify_point(
    alpha: f64,
    jg: f64,
    jl: f64,
    d: f64,
    sin_th: f64,
    cos_th: f64,
    p: f64,
    g: f64,
    fluid: &str,
    t: Option<f64>,
) -> PyResult<u8> {
    let f = fluid_or_err(fluid)?;
    let t = t.unwrap_or(f.t_ref);
    Ok(classify(&FlowPoint {
        alpha,
        jg,
        jl,
        d,
        sin_th,
        cos_th,
        rho_g: f.rho_gas(p, t),
        rho_l: f.rho_liq(p, t),
        mu_g: f.mu_gas(t),
        mu_l: f.mu_liq(t),
        g,
    }) as u8)
}

/// Wood two-phase mixture sound speed [m/s]. `thermal` selects the adiabatic
/// gas sound speed, matching a run with the energy equation switched on.
#[pyfunction]
#[pyo3(signature = (alpha, p, t=None, fluid="air-water", thermal=false))]
fn wood_speed(alpha: f64, p: f64, t: Option<f64>, fluid: &str, thermal: bool) -> PyResult<f64> {
    let f = fluid_or_err(fluid)?;
    Ok(wood_sound_speed(
        alpha,
        p,
        t.unwrap_or(f.t_ref),
        &f,
        thermal,
    ))
}

/// Properties of a named fluid pair, as a dict.
#[pyfunction]
fn fluid_properties<'py>(py: Python<'py>, name: &str) -> PyResult<Bound<'py, PyDict>> {
    let f = fluid_or_err(name)?;
    let d = PyDict::new(py);
    d.set_item("gas_mw", f.gas_mw)?;
    d.set_item("gas_z", f.gas_z)?;
    d.set_item("gas_mu", f.gas_mu)?;
    d.set_item("gas_cp", f.gas_cp)?;
    d.set_item("liq_rho", f.liq_rho)?;
    d.set_item("liq_a", f.liq_a)?;
    d.set_item("liq_beta", f.liq_beta)?;
    d.set_item("liq_mu", f.liq_mu)?;
    d.set_item("liq_mu_b", f.liq_mu_b)?;
    d.set_item("liq_cp", f.liq_cp)?;
    d.set_item("sigma", f.sigma)?;
    d.set_item("p_ref", f.p_ref)?;
    d.set_item("t_ref", f.t_ref)?;
    d.set_item("r_gas", f.r_gas())?;
    d.set_item("gamma", f.gamma())?;
    d.set_item("a_gas_isothermal", f.a_gas2(f.t_ref, false).sqrt())?;
    Ok(d)
}

#[pymodule]
fn phase_flow(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<Sim>()?;
    m.add_function(wrap_pyfunction!(classify_point, m)?)?;
    m.add_function(wrap_pyfunction!(wood_speed, m)?)?;
    m.add_function(wrap_pyfunction!(fluid_properties, m)?)?;
    m.add("FLUIDS", vec!["air-water", "gas-oil", "gas-condensate"])?;
    m.add(
        "REGIME_NAMES",
        vec![
            "stratified-smooth",
            "stratified-wavy",
            "intermittent",
            "annular",
            "dispersed-bubble",
            "bubbly",
            "churn",
            "single-liquid",
            "single-gas",
        ],
    )?;
    Ok(())
}
