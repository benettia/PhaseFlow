//! Design quantities derived from the current state — the numbers an engineer
//! actually sizes a line against, computed from the same state the pipe view
//! is drawn from so the two can never disagree.
//!
//! Everything here is a pure function of the solver state: nothing is
//! integrated over time, so a report taken from a scrubbed-back frame is the
//! report for that instant.

/// A pressure-drop split plus inventory and duty checks.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Report {
    /// Inlet-cell pressure minus outlet-cell pressure [Pa].
    pub dp_total: f64,
    /// Friction contribution to `dp_total` [Pa] (always a loss when flowing
    /// forward; the integral of the Darcy-Weisbach gradient).
    pub dp_friction: f64,
    /// Elevation contribution [Pa]: positive when the line climbs.
    pub dp_gravity: f64,
    /// Whatever `dp_total` has left over — acceleration plus the unsteady
    /// term. Large means the line is not near steady state.
    pub dp_accel: f64,

    /// Mass held in the line right now [kg].
    pub gas_inventory: f64,
    pub liquid_inventory: f64,
    /// Volume-averaged liquid holdup [-].
    pub holdup_avg: f64,

    /// Mass rates crossing the two boundary faces [kg/s], signed with the
    /// flow direction.
    pub w_gas_in: f64,
    pub w_liq_in: f64,
    pub w_gas_out: f64,
    pub w_liq_out: f64,

    /// Largest mixture velocity anywhere [m/s] and the cell it is in.
    pub v_mix_max: f64,
    pub v_mix_max_cell: usize,
    /// Worst API RP 14E erosional-velocity ratio `v_m / v_e`, `v_e = 122/sqrt(rho_m)`
    /// (C = 100, continuous service). Above 1.0 the line is undersized.
    pub erosion_ratio: f64,
    pub erosion_cell: usize,

    pub p_min: f64,
    pub p_max: f64,
    pub t_min: f64,
    pub t_max: f64,
    /// Slowest mixture sound speed in the line [m/s] — what sets the
    /// water-hammer surge pressure and the solver's own time step.
    pub a_min: f64,
}

/// API RP 14E erosional velocity limit [m/s] at mixture density `rho_m`,
/// C = 100 in the original ft/s, lb/ft3 units.
pub fn erosional_velocity(rho_m: f64) -> f64 {
    122.0 / rho_m.max(1.0e-6).sqrt()
}
