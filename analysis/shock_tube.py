"""Pure-gas shock tube vs exact isothermal Riemann solution.

Acceptance: shock speed within 1 % of exact.
Run: uv run analysis/shock_tube.py  (writes analysis/out/shock_tube.png)
"""

import matplotlib

matplotlib.use("Agg")
import matplotlib.pyplot as plt
import numpy as np
from common import OUT, isothermal_riemann, make_sim, riemann_profile, shock_tube_scenario

PL, PR, N = 4e5, 1e5, 400


def run():
    sim = make_sim(shock_tube_scenario(N, PL, PR))
    _, _, s_exact = isothermal_riemann(PL, PR)
    thresh = 0.5 * (PR + 1.99e5)

    def front():
        x, p = sim.x, sim.p
        idx = np.where(p > thresh)[0]
        i = idx[-1]
        if i + 1 < len(p) and p[i + 1] < p[i]:
            f = (p[i] - thresh) / (p[i] - p[i + 1])
            return x[i] + f * (x[i + 1] - x[i])
        return x[i]

    sim.run(0.03)
    t1, x1 = sim.time, front()
    sim.run(0.03)
    t2, x2 = sim.time, front()
    s_sim = (x2 - x1) / (t2 - t1)
    return sim, s_sim, s_exact


def test_shock_speed():
    _, s_sim, s_exact = run()
    assert abs(s_sim - s_exact) / s_exact < 0.01, f"{s_sim:.2f} vs {s_exact:.2f}"


if __name__ == "__main__":
    sim, s_sim, s_exact = run()
    fig, ax = plt.subplots(figsize=(9, 4))
    ax.plot(sim.x, sim.p / 1e3, lw=1.1, label=f"AUSMV N={N}, t={sim.time:.3f}s")
    ax.plot(sim.x, riemann_profile(sim.x, sim.time, PL, PR) / 1e3, "k--", lw=1, label="exact")
    ax.set(
        xlabel="x [m]",
        ylabel="p [kPa]",
        title=f"isothermal shock tube — shock speed {s_sim:.1f} vs exact {s_exact:.1f} m/s "
        f"({100 * abs(s_sim - s_exact) / s_exact:.2f} %)",
    )
    ax.legend()
    fig.tight_layout()
    fig.savefig(f"{OUT}/shock_tube.png", dpi=140)
    print(f"shock speed: sim {s_sim:.2f}, exact {s_exact:.2f} m/s")
    print(f"wrote {OUT}/shock_tube.png")
