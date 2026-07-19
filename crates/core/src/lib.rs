//! phase-core: isothermal 1D drift-flux multiphase pipe flow solver.
//!
//! Conserved state per cell U = [m_g, m_l, I] with m_k = alpha_k * rho_k and
//! I the mixture momentum. Closed by a Zuber-Findlay slip law, isothermal EOS
//! for both phases, Churchill wall friction, and AUSMV flux splitting
//! (Evje-Fjelde) with optional MUSCL/MC second-order reconstruction.
//!
//! Determinism: f64 everywhere, no mul_add, no randomness, fixed iteration
//! counts wherever a solve appears. Same scenario + fixed dt => identical
//! trajectory on a given build.
#![forbid(unsafe_code)]

pub mod closures;
pub mod eos;
pub mod regime;
pub mod scenario;
pub mod sim;

pub use regime::{classify, Regime};
pub use scenario::{InitState, Inlet, Options, Outlet, Scenario, Segment};
pub use sim::{Sim, SimError};
