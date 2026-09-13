//! JSON <-> phase_core::Scenario. Shared by the wasm and python wrappers so
//! the zero-dep core never sees serde.

use phase_core::fluid::Fluid;
use phase_core::scenario::DEFAULT_ROUGHNESS;
use phase_core::{InitState, Inlet, Options, Outlet, Scenario, Segment};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Copy)]
pub struct InitJson {
    pub alpha_g: f64,
    pub p: f64,
    #[serde(default)]
    pub v: f64,
    #[serde(default)]
    pub t: Option<f64>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct SegmentJson {
    pub length: f64,
    pub angle: f64, // degrees from horizontal, +uphill
    pub diameter: f64,
    pub cells: usize,
    #[serde(default)]
    pub init: Option<InitJson>,
    #[serde(default)]
    pub roughness: Option<f64>,
    #[serde(default)]
    pub u_wall: Option<f64>,
}

#[derive(Serialize, Deserialize, Clone, Copy)]
pub struct InletJson {
    pub wg: f64,
    pub wl: f64,
    #[serde(default)]
    pub p_anchor: Option<f64>,
    #[serde(default)]
    pub makeup_alpha: Option<f64>,
    #[serde(default)]
    pub t: Option<f64>,
}

#[derive(Serialize, Deserialize, Clone, Copy)]
pub struct OutletJson {
    pub p: f64,
    #[serde(default = "one")]
    pub choke: f64,
    #[serde(default = "default_cv")]
    pub cv: f64,
}

fn one() -> f64 {
    1.0
}
fn default_cv() -> f64 {
    0.5
}
fn default_cfl() -> f64 {
    0.5
}
fn tru() -> bool {
    true
}
fn default_g() -> f64 {
    9.81
}
fn default_roughness() -> f64 {
    DEFAULT_ROUGHNESS
}

#[derive(Serialize, Deserialize, Clone, Copy)]
pub struct OptionsJson {
    #[serde(default = "tru")]
    pub muscl: bool,
    #[serde(default = "default_cfl")]
    pub cfl: f64,
    #[serde(default)]
    pub fixed_dt: Option<f64>,
    #[serde(default = "tru")]
    pub wall_friction: bool,
    #[serde(default = "default_g")]
    pub g: f64,
    #[serde(default)]
    pub regime_feedback: bool,
    #[serde(default)]
    pub hydrostatic_init: bool,
    #[serde(default)]
    pub thermal: bool,
    #[serde(default)]
    pub t_ambient: Option<f64>,
    #[serde(default)]
    pub u_wall: f64,
    #[serde(default = "default_roughness")]
    pub roughness: f64,
}

impl Default for OptionsJson {
    fn default() -> Self {
        serde_json::from_str("{}").unwrap()
    }
}

/// Fluid block: a named pair plus any per-property override. Anything absent
/// keeps the preset value, so `{"preset": "gas-oil", "liq_mu": 0.02}` is a
/// heavy crude in an otherwise standard flowline.
#[derive(Serialize, Deserialize, Clone, Default)]
pub struct FluidJson {
    #[serde(default)]
    pub preset: Option<String>,
    #[serde(default)]
    pub gas_mw: Option<f64>,
    #[serde(default)]
    pub gas_z: Option<f64>,
    #[serde(default)]
    pub gas_mu: Option<f64>,
    #[serde(default)]
    pub gas_cp: Option<f64>,
    #[serde(default)]
    pub liq_rho: Option<f64>,
    #[serde(default)]
    pub liq_a: Option<f64>,
    #[serde(default)]
    pub liq_beta: Option<f64>,
    #[serde(default)]
    pub liq_mu: Option<f64>,
    #[serde(default)]
    pub liq_mu_b: Option<f64>,
    #[serde(default)]
    pub liq_cp: Option<f64>,
    #[serde(default)]
    pub sigma: Option<f64>,
    #[serde(default)]
    pub p_ref: Option<f64>,
    #[serde(default)]
    pub t_ref: Option<f64>,
}

impl FluidJson {
    fn resolve(&self) -> Result<Fluid, String> {
        let mut f = match &self.preset {
            Some(name) => {
                Fluid::by_name(name).ok_or_else(|| format!("unknown fluid preset '{name}'"))?
            }
            None => Fluid::default(),
        };
        macro_rules! set {
            ($($field:ident),* $(,)?) => {$(
                if let Some(v) = self.$field { f.$field = v; }
            )*};
        }
        set!(
            gas_mw, gas_z, gas_mu, gas_cp, liq_rho, liq_a, liq_beta, liq_mu, liq_mu_b, liq_cp,
            sigma, p_ref, t_ref
        );
        if !(f.gas_mw > 0.0 && f.gas_z > 0.0 && f.liq_rho > 0.0 && f.liq_a > 0.0 && f.t_ref > 0.0) {
            return Err(
                "fluid has a non-positive molar mass, density, sound speed or T_ref".into(),
            );
        }
        if !(f.gas_cp > 0.0 && f.liq_cp > 0.0 && f.gas_mu > 0.0 && f.liq_mu > 0.0) {
            return Err("fluid has a non-positive heat capacity or viscosity".into());
        }
        Ok(f)
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct ScenarioJson {
    #[serde(default)]
    pub name: String,
    pub segments: Vec<SegmentJson>,
    pub init: InitJson,
    pub inlet: InletJson,
    pub outlet: OutletJson,
    #[serde(default)]
    pub options: OptionsJson,
    #[serde(default)]
    pub fluid: FluidJson,
}

fn init_of(j: InitJson) -> InitState {
    InitState {
        alpha_g: j.alpha_g,
        p: j.p,
        v: j.v,
        t: j.t,
    }
}

pub fn parse(json: &str) -> Result<Scenario, String> {
    let sj: ScenarioJson = serde_json::from_str(json).map_err(|e| e.to_string())?;
    if sj.segments.is_empty() {
        return Err("scenario has no segments".into());
    }
    let fluid = sj.fluid.resolve()?;
    Ok(Scenario {
        segments: sj
            .segments
            .iter()
            .map(|s| Segment {
                length: s.length,
                angle_deg: s.angle,
                diameter: s.diameter,
                cells: s.cells,
                init: s.init.map(init_of),
                roughness: s.roughness,
                u_wall: s.u_wall,
            })
            .collect(),
        init: init_of(sj.init),
        inlet: Inlet {
            wg: sj.inlet.wg,
            wl: sj.inlet.wl,
            p_anchor: sj.inlet.p_anchor,
            makeup_alpha: sj.inlet.makeup_alpha,
            t: sj.inlet.t,
        },
        outlet: Outlet {
            p: sj.outlet.p,
            choke: sj.outlet.choke,
            cv: sj.outlet.cv,
        },
        options: Options {
            muscl: sj.options.muscl,
            cfl: sj.options.cfl,
            fixed_dt: sj.options.fixed_dt,
            wall_friction: sj.options.wall_friction,
            g: sj.options.g,
            regime_feedback: sj.options.regime_feedback,
            hydrostatic_init: sj.options.hydrostatic_init,
            thermal: sj.options.thermal,
            t_ambient: sj.options.t_ambient,
            u_wall: sj.options.u_wall,
            roughness: sj.options.roughness,
        },
        fluid,
    })
}
