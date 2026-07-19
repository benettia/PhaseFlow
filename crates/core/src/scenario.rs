//! Scenario description: plain structs, no serde here (core stays zero-dep).
//! JSON parsing lives in crates/scenario, shared by the wasm and python wrappers.

#[derive(Clone, Copy, Debug)]
pub struct InitState {
    pub alpha_g: f64,
    pub p: f64,
    pub v: f64, // initial common phase velocity [m/s]
}

impl Default for InitState {
    fn default() -> Self {
        Self {
            alpha_g: 0.5,
            p: 1.0e5,
            v: 0.0,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Segment {
    pub length: f64,    // [m]
    pub angle_deg: f64, // from horizontal, +uphill in flow direction
    pub diameter: f64,  // [m]
    pub cells: usize,
    pub init: Option<InitState>, // override of Scenario::init
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Inlet {
    pub wg: f64, // gas mass rate [kg/s]
    pub wl: f64, // liquid mass rate [kg/s]
    /// Fixed inlet-face pressure. None extrapolates from the first cell
    /// (mass-flux-only inlet). Some(p) models a feed connected to a large
    /// reservoir at p — the water-faucet top open to the atmosphere.
    pub p_anchor: Option<f64>,
    /// Gas make-up inlet (requires p_anchor): ignore wg and let gas flow in
    /// at reservoir density with whatever velocity the first cell demands,
    /// holding this void fraction at the feed. This is how an open faucet
    /// top behaves: air replaces the liquid that falls away.
    pub makeup_alpha: Option<f64>,
}

#[derive(Clone, Copy, Debug)]
pub struct Outlet {
    pub p: f64,     // reservoir pressure behind the choke [Pa]
    pub choke: f64, // opening 0..1
    pub cv: f64,    // discharge coefficient (quadratic valve law)
}

#[derive(Clone, Copy, Debug)]
pub struct Options {
    pub muscl: bool,
    pub cfl: f64,
    pub fixed_dt: Option<f64>, // Some => deterministic fixed-dt mode
    pub wall_friction: bool,
    pub g: f64,
    pub regime_feedback: bool, // regime modulates C0/v_d (default off)
    pub hydrostatic_init: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            muscl: true,
            cfl: 0.5,
            fixed_dt: None,
            wall_friction: true,
            g: 9.81,
            regime_feedback: false,
            hydrostatic_init: false,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Scenario {
    pub segments: Vec<Segment>,
    pub init: InitState,
    pub inlet: Inlet,
    pub outlet: Outlet,
    pub options: Options,
}
