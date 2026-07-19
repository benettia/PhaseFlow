//! pyo3 bindings: `import phase_flow`. Arrays come out as numpy copies —
//! never aliased mutable views into live solver state (safe by construction;
//! a copy of a few KB per call is nothing next to the step cost).

use numpy::{PyArray1, PyArrayMethods};
use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;

use phase_core::eos::{rho_gas, rho_liq, wood_sound_speed};
use phase_core::{classify, SimError};

fn err(e: SimError) -> PyErr {
    match e {
        SimError::NanAtCell(i) => PyRuntimeError::new_err(format!("NaN detected in cell {i}")),
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

    /// (gas_mass, liquid_mass) totals [kg].
    fn mass_totals(&self) -> (f64, f64) {
        self.inner.mass_totals()
    }
}

/// Classify a flow point; returns the regime code 0..8.
#[pyfunction]
#[pyo3(signature = (alpha, jg, jl, d, sin_th=0.0, cos_th=1.0, p=1.0e5, g=9.81))]
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
) -> u8 {
    classify(alpha, jg, jl, d, sin_th, cos_th, rho_gas(p), rho_liq(p), g) as u8
}

/// Wood two-phase mixture sound speed [m/s].
#[pyfunction]
fn wood_speed(alpha: f64, p: f64) -> f64 {
    wood_sound_speed(alpha, p)
}

#[pymodule]
fn phase_flow(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<Sim>()?;
    m.add_function(wrap_pyfunction!(classify_point, m)?)?;
    m.add_function(wrap_pyfunction!(wood_speed, m)?)?;
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
