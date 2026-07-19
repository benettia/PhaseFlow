//! JSON <-> phase_core::Scenario. Shared by the wasm and python wrappers so
//! the zero-dep core never sees serde.

use phase_core::{InitState, Inlet, Options, Outlet, Scenario, Segment};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Copy)]
pub struct InitJson {
    pub alpha_g: f64,
    pub p: f64,
    #[serde(default)]
    pub v: f64,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct SegmentJson {
    pub length: f64,
    pub angle: f64, // degrees from horizontal, +uphill
    pub diameter: f64,
    pub cells: usize,
    #[serde(default)]
    pub init: Option<InitJson>,
}

#[derive(Serialize, Deserialize, Clone, Copy)]
pub struct InletJson {
    pub wg: f64,
    pub wl: f64,
    #[serde(default)]
    pub p_anchor: Option<f64>,
    #[serde(default)]
    pub makeup_alpha: Option<f64>,
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
}

impl Default for OptionsJson {
    fn default() -> Self {
        serde_json::from_str("{}").unwrap()
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
}

fn init_of(j: InitJson) -> InitState {
    InitState {
        alpha_g: j.alpha_g,
        p: j.p,
        v: j.v,
    }
}

pub fn parse(json: &str) -> Result<Scenario, String> {
    let sj: ScenarioJson = serde_json::from_str(json).map_err(|e| e.to_string())?;
    if sj.segments.is_empty() {
        return Err("scenario has no segments".into());
    }
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
            })
            .collect(),
        init: init_of(sj.init),
        inlet: Inlet {
            wg: sj.inlet.wg,
            wl: sj.inlet.wl,
            p_anchor: sj.inlet.p_anchor,
            makeup_alpha: sj.inlet.makeup_alpha,
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
        },
    })
}
