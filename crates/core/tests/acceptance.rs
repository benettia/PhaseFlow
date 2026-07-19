//! Fast acceptance + invariant tests. The heavier verification studies
//! (convergence plots, slugging cycle analysis, valve slam wave tracking)
//! live in analysis/*.py against the python bindings.

use phase_core::closures::{c0, drift_velocity, phase_velocities};
use phase_core::eos::{pressure_from_masses, rho_gas, rho_liq, wood_sound_speed};
use phase_core::regime::Regime;
use phase_core::{classify, InitState, Inlet, Options, Outlet, Scenario, Segment, Sim};

fn scenario(segments: Vec<Segment>, init: InitState) -> Scenario {
    Scenario {
        segments,
        init,
        inlet: Inlet {
            wg: 0.0,
            wl: 0.0,
            p_anchor: None,
            makeup_alpha: None,
        },
        outlet: Outlet {
            p: 1.0e5,
            choke: 0.0,
            cv: 0.5,
        },
        options: Options::default(),
    }
}

#[test]
fn primitives_round_trip() {
    for &p in &[2.0e4, 1.0e5, 8.0e5, 5.0e6] {
        for &a in &[1.0e-6, 0.01, 0.3, 0.5, 0.9, 0.999999] {
            let mg = a * rho_gas(p);
            let ml = (1.0 - a) * rho_liq(p);
            let pr = pressure_from_masses(mg, ml);
            assert!((pr - p).abs() / p < 1.0e-10, "p={p} a={a} recovered {pr}");
        }
    }
}

#[test]
fn velocity_solve_round_trip() {
    // pick (vg, vl), derive (mom, vd) consistent with the slip law, solve back
    for &a in &[0.05, 0.3, 0.6, 0.95] {
        let p = 2.0e5;
        let (rg, rl) = (rho_gas(p), rho_liq(p));
        let (mg, ml) = (a * rg, (1.0 - a) * rl);
        let (vg_ref, vl_ref) = (2.0, 0.7);
        let j = a * vg_ref + (1.0 - a) * vl_ref;
        let c = c0(a);
        let vd = vg_ref - c * j;
        let mom = mg * vg_ref + ml * vl_ref;
        let (vg, vl) = phase_velocities(mg, ml, mom, a, c, vd);
        assert!((vg - vg_ref).abs() < 1.0e-9, "a={a} vg={vg}");
        assert!((vl - vl_ref).abs() < 1.0e-9, "a={a} vl={vl}");
    }
}

#[test]
fn wood_speed_dips_at_intermediate_void() {
    let p = 1.0e5;
    let a_half = wood_sound_speed(0.5, p);
    // classic result: ~20 m/s for air-water at 1 bar, far below both phases
    assert!(a_half > 15.0 && a_half < 30.0, "a_m(0.5)={a_half}");
    assert!(wood_sound_speed(1.0 - 1e-6, p) > 300.0);
    assert!(wood_sound_speed(1e-6, p) > 900.0);
}

#[test]
fn drift_vanishes_at_pure_gas() {
    let vd = drift_velocity(1.0 - 1e-9, 1.2, 1000.0, 1.0, 9.81);
    assert!(vd.abs() < 1.0e-6);
}

#[test]
fn classifier_sanity_horizontal_air_water() {
    let (d, rg, rl, g) = (0.05, 1.2, 1000.0, 9.81);
    // low rates -> stratified
    let r = classify(0.5, 0.5, 0.05, d, 0.0, 1.0, rg, rl, g);
    assert!(
        r == Regime::StratifiedSmooth || r == Regime::StratifiedWavy,
        "low rates gave {r:?}"
    );
    // high liquid, modest gas -> intermittent or dispersed
    let r = classify(0.3, 1.0, 3.0, d, 0.0, 1.0, rg, rl, g);
    assert!(
        r == Regime::Intermittent || r == Regime::DispersedBubble,
        "slug region gave {r:?}"
    );
    // very high gas, little liquid -> annular
    let r = classify(0.95, 25.0, 0.05, d, 0.0, 1.0, rg, rl, g);
    assert_eq!(r, Regime::Annular);
    // vertical thresholds
    assert_eq!(
        classify(0.1, 0.2, 1.0, d, 1.0, 0.0, rg, rl, g),
        Regime::Bubbly
    );
    assert_eq!(
        classify(0.4, 1.0, 1.0, d, 1.0, 0.0, rg, rl, g),
        Regime::Intermittent
    );
    assert_eq!(
        classify(0.9, 10.0, 0.1, d, 1.0, 0.0, rg, rl, g),
        Regime::Annular
    );
}

/// Acceptance 3: closed ends, sloshing contents — each phase's mass constant
/// to 1e-12 relative over 1000 steps.
#[test]
fn mass_conservation_closed_ends() {
    let mut sc = scenario(
        vec![
            Segment {
                length: 5.0,
                angle_deg: -20.0,
                diameter: 0.1,
                cells: 25,
                init: Some(InitState {
                    alpha_g: 0.7,
                    p: 2.0e5,
                    v: 0.0,
                }),
            },
            Segment {
                length: 5.0,
                angle_deg: 20.0,
                diameter: 0.1,
                cells: 25,
                init: Some(InitState {
                    alpha_g: 0.2,
                    p: 1.0e5,
                    v: 0.0,
                }),
            },
        ],
        InitState::default(),
    );
    sc.options.muscl = true;
    let mut sim = Sim::new(&sc).unwrap();
    let (g0, l0) = sim.mass_totals();
    for _ in 0..1000 {
        sim.step().unwrap();
    }
    let (g1, l1) = sim.mass_totals();
    let (dg, dl) = ((g1 - g0) / g0, (l1 - l0) / l0);
    assert!(dg.abs() < 1.0e-12, "gas drift {dg:e}");
    assert!(dl.abs() < 1.0e-12, "liq drift {dl:e}");
}

/// Acceptance 6: fixed-dt mode is bit-deterministic.
#[test]
fn fixed_dt_bit_determinism() {
    let mut sc = scenario(
        vec![Segment {
            length: 10.0,
            angle_deg: 5.0,
            diameter: 0.08,
            cells: 50,
            init: None,
        }],
        InitState {
            alpha_g: 0.4,
            p: 1.5e5,
            v: 1.0,
        },
    );
    sc.inlet = Inlet {
        wg: 0.005,
        wl: 0.5,
        p_anchor: None,
        makeup_alpha: None,
    };
    sc.outlet = Outlet {
        p: 1.0e5,
        choke: 0.8,
        cv: 0.5,
    };
    sc.options.fixed_dt = Some(5.0e-5);
    let run = || {
        let mut sim = Sim::new(&sc).unwrap();
        for _ in 0..500 {
            sim.step().unwrap();
        }
        (sim.p().to_vec(), sim.alpha().to_vec(), sim.vg().to_vec())
    };
    let (p1, a1, v1) = run();
    let (p2, a2, v2) = run();
    assert!(p1.iter().zip(&p2).all(|(x, y)| x.to_bits() == y.to_bits()));
    assert!(a1.iter().zip(&a2).all(|(x, y)| x.to_bits() == y.to_bits()));
    assert!(v1.iter().zip(&v2).all(|(x, y)| x.to_bits() == y.to_bits()));
}

/// Exact shock speed for the isothermal Euler Riemann problem (right shock):
/// solve for post-shock density from the jump conditions.
fn isothermal_shock_speed(rho_l: f64, rho_r: f64, a: f64) -> f64 {
    // left rarefaction + right shock, both states at rest.
    // middle state: u* = -a ln(r*/rho_l)  (rarefaction from left)
    //               u* = (r* - rho_r) * a / sqrt(r* * rho_r)  (shock)
    let mut lo = rho_r;
    let mut hi = rho_l;
    for _ in 0..200 {
        let r = 0.5 * (lo + hi);
        let u_rar = -a * (r / rho_l).ln();
        let u_shk = (r - rho_r) * a / (r * rho_r).sqrt();
        if u_rar > u_shk {
            lo = r;
        } else {
            hi = r;
        }
    }
    let r = 0.5 * (lo + hi);
    let u_star = (r - rho_r) * a / (r * rho_r).sqrt();
    // shock speed from mass conservation: s = (r*u* - 0) / (r - rho_r)
    r * u_star / (r - rho_r)
}

/// Acceptance 2: pure-gas shock tube, shock speed within 1% of exact.
#[test]
fn gas_shock_tube_speed() {
    let a_g = 316.0;
    let (pl, pr) = (4.0e5, 1.0e5);
    let n = 400;
    let mut sc = scenario(
        vec![
            Segment {
                length: 50.0,
                angle_deg: 0.0,
                diameter: 0.1,
                cells: n / 2,
                init: Some(InitState {
                    alpha_g: 1.0 - 1e-6,
                    p: pl,
                    v: 0.0,
                }),
            },
            Segment {
                length: 50.0,
                angle_deg: 0.0,
                diameter: 0.1,
                cells: n / 2,
                init: Some(InitState {
                    alpha_g: 1.0 - 1e-6,
                    p: pr,
                    v: 0.0,
                }),
            },
        ],
        InitState::default(),
    );
    sc.options.wall_friction = false;
    sc.options.g = 0.0;
    sc.options.muscl = true;
    let mut sim = Sim::new(&sc).unwrap();
    // measure front displacement between two times: cancels the constant
    // smearing offset of any single-threshold detection
    let front = |sim: &Sim, thresh: f64| -> f64 {
        let (x, p) = (sim.x_mid(), sim.p());
        for i in (1..n).rev() {
            if p[i] > thresh {
                // linear interpolation of the crossing inside [i, i+1]
                if i + 1 < n && p[i + 1] < p[i] {
                    let f = (p[i] - thresh) / (p[i] - p[i + 1]);
                    return x[i] + f * (x[i + 1] - x[i]);
                }
                return x[i];
            }
        }
        0.0
    };
    let s_exact = isothermal_shock_speed(pl / (a_g * a_g), pr / (a_g * a_g), a_g);
    // mid-jump threshold between p_r and the exact star pressure
    let thresh = 0.5 * (pr + 1.98 * 1.0e5);
    sim.advance(0.03).unwrap();
    let (t1, x1) = (sim.time(), front(&sim, thresh));
    sim.advance(0.03).unwrap();
    let (t2, x2) = (sim.time(), front(&sim, thresh));
    let s_sim = (x2 - x1) / (t2 - t1);
    let err = (s_sim - s_exact).abs() / s_exact;
    assert!(
        err < 0.01,
        "shock speed sim {s_sim:.2} vs exact {s_exact:.2} m/s, err {:.3}%",
        err * 100.0
    );
}

/// Ransom water faucet analytic void profile at time t.
fn faucet_alpha_exact(x: f64, t: f64, v0: f64, al0: f64, g: f64) -> f64 {
    if x <= v0 * t + 0.5 * g * t * t {
        1.0 - al0 * v0 / (v0 * v0 + 2.0 * g * x).sqrt()
    } else {
        1.0 - al0
    }
}

/// Acceptance 1 (fast version): faucet L1 error under tolerance at t=0.5 s
/// and shrinking with refinement at observed order >= 0.8.
/// The full convergence study with plots is analysis/faucet.py.
#[test]
fn water_faucet_l1_and_order() {
    let run = |cells: usize| -> Vec<f64> {
        let mut sc = scenario(
            vec![Segment {
                length: 12.0,
                angle_deg: -90.0,
                diameter: 1.0,
                cells,
                init: None,
            }],
            InitState {
                alpha_g: 0.2,
                p: 1.0e5,
                v: 10.0,
            },
        );
        let area = 0.25 * std::f64::consts::PI;
        sc.inlet = Inlet {
            wg: 0.0, // replaced by makeup feed
            wl: 0.8 * 1000.0 * 10.0 * area,
            // top of the column open to the atmosphere: without the anchor the
            // fixed-mass-flux inlet lets the column hang on a suction gradient
            p_anchor: Some(1.0e5),
            makeup_alpha: Some(0.2),
        };
        sc.outlet = Outlet {
            p: 1.0e5,
            choke: 1.0,
            cv: 20.0,
        };
        sc.options.wall_friction = false;
        // fixed dt so every resolution integrates to exactly t = 0.5
        sc.options.fixed_dt = Some(0.5 / (2000 * cells / 48) as f64);
        let mut sim = Sim::new(&sc).unwrap();
        sim.advance(0.5 - 1e-9).unwrap();
        sim.alpha().to_vec()
    };
    // restrict a 2N-cell profile onto N cells by averaging pairs
    let restrict =
        |fine: &[f64]| -> Vec<f64> { fine.chunks(2).map(|c| 0.5 * (c[0] + c[1])).collect() };
    let l1 = |a: &[f64], b: &[f64]| -> f64 {
        a.iter().zip(b).map(|(x, y)| (x - y).abs()).sum::<f64>() / a.len() as f64
    };
    let (a48, a96, a192) = (run(48), run(96), run(192));
    // (a) distance to the Ransom analytic solution. A strict drift-flux model
    // cannot reproduce the two-fluid faucet exactly: pressure information
    // travels at the Wood speed (~25 m/s at alpha=0.2), so the column cannot
    // stay isobaric like the analytic solution assumes. The tolerance below
    // is the documented model-error floor (see analysis/faucet.py for the
    // side-by-side plot), not loose numerics.
    let exact48: Vec<f64> = (0..48)
        .map(|i| faucet_alpha_exact((i as f64 + 0.5) * 0.25, 0.5, 10.0, 0.8, 9.81))
        .collect();
    let e_analytic = l1(&restrict(&restrict(&a192)), &exact48);
    assert!(
        e_analytic < 0.08,
        "L1 vs analytic at N=192: {e_analytic:.4}"
    );
    // (b) numerics verification: self-convergence at observed order >= 0.8
    let c1 = l1(&restrict(&a96), &a48);
    let c2 = l1(&restrict(&a192), &a96);
    let order = (c1 / c2).ln() / 2.0f64.ln();
    assert!(
        order >= 0.8,
        "observed order {order:.2} (cauchy {c1:.5} -> {c2:.5})"
    );
}
