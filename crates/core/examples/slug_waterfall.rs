//! ASCII waterfall of void fraction along the pipe over time.
use phase_core::{InitState, Inlet, Options, Outlet, Scenario, Segment, Sim};

fn main() {
    let args: Vec<f64> = std::env::args()
        .skip(1)
        .map(|a| a.parse().unwrap())
        .collect();
    let wl = args.first().copied().unwrap_or(2.0);
    let wg = args.get(1).copied().unwrap_or(0.0005);
    let t_end = args.get(2).copied().unwrap_or(400.0);
    let sc = Scenario {
        segments: vec![
            Segment {
                length: 80.0,
                angle_deg: -4.0,
                diameter: 0.08,
                cells: 120,
                init: Some(InitState {
                    alpha_g: 0.6,
                    p: 2.0e5,
                    v: 0.0,
                }),
            },
            Segment {
                length: 12.0,
                angle_deg: 90.0,
                diameter: 0.08,
                cells: 36,
                init: Some(InitState {
                    alpha_g: 0.03,
                    p: 2.0e5,
                    v: 0.0,
                }),
            },
        ],
        init: InitState::default(),
        inlet: Inlet {
            wg,
            wl,
            p_anchor: None,
            makeup_alpha: None,
        },
        outlet: Outlet {
            p: 1.0e5,
            choke: 1.0,
            cv: 0.6,
        },
        options: Options {
            hydrostatic_init: true,
            regime_feedback: true,
            ..Default::default()
        },
    };
    let mut sim = Sim::new(&sc).unwrap();
    let glyph = |a: f64| -> char {
        match (a * 10.0) as usize {
            0 => '.',
            1..=2 => ':',
            3..=4 => 'o',
            5..=6 => 'O',
            7..=8 => '#',
            _ => '@',
        }
    };
    println!("      |pipeline (60m, -3deg) -> riser (15m)|  p_base[kPa]");
    while sim.time() < t_end {
        sim.advance(5.0).unwrap();
        let a = sim.alpha();
        let mut line = String::new();
        for i in (0..156).step_by(3) {
            line.push(glyph(a[i]));
        }
        println!("{:6.0} {} {:6.1}", sim.time(), line, sim.p()[120] / 1e3);
    }
}
