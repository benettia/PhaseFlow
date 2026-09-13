//! phase-core: 1D drift-flux multiphase pipe flow solver.
//!
//! Conserved state per cell U = [m_g, m_l, I, e] with m_k = alpha_k * rho_k,
//! I the mixture momentum and e the mixture internal energy (carried only
//! when the energy equation is switched on; otherwise the model is strictly
//! isothermal). Closed by a Zuber-Findlay slip law, a compressible p-T
//! equation of state for both phases, Churchill wall friction, and AUSMV flux
//! splitting (Evje-Fjelde) with optional MUSCL/MC second-order
//! reconstruction. Fluid properties are data (see `fluid::Fluid`), not
//! constants baked into the solver.
//!
//! Determinism: f64 everywhere, no mul_add, no randomness, fixed iteration
//! counts wherever a solve appears. Same scenario + fixed dt => identical
//! trajectory on a given build.
#![forbid(unsafe_code)]

pub mod closures;
pub mod eos;
pub mod fluid;
pub mod regime;
pub mod report;
pub mod scenario;
pub mod sim;

pub use fluid::Fluid;
pub use regime::{classify, FlowPoint, Regime};
pub use report::Report;
pub use scenario::{InitState, Inlet, Options, Outlet, Scenario, Segment};
pub use sim::{Sim, SimError};
