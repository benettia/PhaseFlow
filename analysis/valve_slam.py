"""Valve slam: choke closed in 0.1 s, the compression wave must travel at the
Wood mixture sound speed for the prevailing void fraction.

Run: uv run analysis/valve_slam.py  (writes analysis/out/valve_slam.png)
"""

import matplotlib

matplotlib.use("Agg")
import matplotlib.pyplot as plt
import numpy as np
import phase_flow as pf
from common import OUT, make_sim, valve_slam_scenario

SETTLE = 25.0
PROBE_A, PROBE_B = 150, 90  # cells at x = 75.25, 45.25 m (dx = 0.5)


def run():
    sim = make_sim(valve_slam_scenario())
    sim.run(SETTLE)  # reach quasi-steady flow
    x = sim.x
    alpha0 = sim.alpha_g.copy()
    p0 = sim.p.copy()
    sc = valve_slam_scenario()
    # slam: choke 1 -> 0 over 0.1 s of sim time
    t_slam = sim.time
    arrivals = {}
    ts, pa, pb = [], [], []
    snapshots = []
    while sim.time < t_slam + 3.0:
        f = max(0.0, 1.0 - (sim.time - t_slam) / 0.1)
        sim.set_bc(sc["inlet"]["wg"], sc["inlet"]["wl"], sc["outlet"]["p"], f)
        sim.run(0.004)
        t = sim.time - t_slam
        p = sim.p
        ts.append(t)
        pa.append(p[PROBE_A])
        pb.append(p[PROBE_B])
        for probe, _series in (("A", pa), ("B", pb)):
            i = PROBE_A if probe == "A" else PROBE_B
            if probe not in arrivals and p[i] > p0[i] + 8e3:
                arrivals[probe] = t
        if len(snapshots) < 6 and t > 0.3 * (len(snapshots) + 1):
            snapshots.append((t, p.copy()))
    dt_wave = arrivals["B"] - arrivals["A"]
    dx_wave = x[PROBE_A] - x[PROBE_B]
    s_sim = dx_wave / dt_wave
    seg = slice(PROBE_B, PROBE_A + 1)
    a_wood = pf.wood_speed(float(np.mean(alpha0[seg])), float(np.mean(p0[seg])))
    # wave rides on the (counter-flowing) medium: v ~ -1 m/s relative
    v_med = float(np.mean(1 - alpha0[seg]) and 0.0)
    return (np.array(ts), np.array(pa), np.array(pb), snapshots, x, s_sim, a_wood, arrivals, v_med)


def test_wave_speed_is_wood():
    *_, s_sim, a_wood, _, _ = run()
    assert abs(s_sim - a_wood) / a_wood < 0.15, f"wave {s_sim:.1f} vs Wood {a_wood:.1f} m/s"


if __name__ == "__main__":
    ts, pa, pb, snaps, x, s_sim, a_wood, arr, _ = run()
    fig, (ax1, ax2) = plt.subplots(1, 2, figsize=(11, 4.2))
    ax1.plot(ts, pa / 1e3, label=f"probe x=75 m (hit {arr['A'] * 1e3:.0f} ms)", color="#1c1b18")
    ax1.plot(ts, pb / 1e3, label=f"probe x=45 m (hit {arr['B'] * 1e3:.0f} ms)", color="#a4442c")
    ax1.set(
        xlabel="t since slam [s]",
        ylabel="p [kPa]",
        title=f"wave speed {s_sim:.1f} m/s vs Wood {a_wood:.1f} m/s "
        f"({100 * abs(s_sim - a_wood) / a_wood:.1f} %)",
    )
    ax1.legend(fontsize=8)
    for t, p in snaps:
        ax2.plot(x, p / 1e3, lw=0.9, label=f"t+{t:.2f}s")
    ax2.set(xlabel="x [m]", ylabel="p [kPa]", title="pressure wave marching upstream")
    ax2.legend(fontsize=7)
    fig.tight_layout()
    fig.savefig(f"{OUT}/valve_slam.png", dpi=140)
    print(f"wave {s_sim:.1f} m/s, Wood {a_wood:.1f} m/s")
    print(f"wrote {OUT}/valve_slam.png")
