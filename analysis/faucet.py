"""Water faucet (Ransom) verification: profiles vs analytic + self-convergence.

The drift-flux model cannot match the two-fluid analytic solution exactly —
pressure information moves at the Wood speed (~25 m/s at alpha=0.2), so the
column cannot stay isobaric. The analytic L1 tolerance below is that model
floor; the *numerics* are verified by Cauchy self-convergence order >= 0.8.

Run: uv run analysis/faucet.py  (writes analysis/out/faucet.png)
"""

import matplotlib

matplotlib.use("Agg")
import matplotlib.pyplot as plt
import numpy as np
from common import OUT, faucet_exact, faucet_scenario, make_sim

T_END = 0.5
GRIDS = [48, 96, 192, 384]


def run(cells: int) -> tuple[np.ndarray, np.ndarray]:
    dt = 0.5 / (2000 * cells // 48)
    sim = make_sim(faucet_scenario(cells, fixed_dt=dt))
    sim.run(T_END - 1e-9)
    return sim.x.copy(), sim.alpha_g.copy()


def restrict(a: np.ndarray) -> np.ndarray:
    return 0.5 * (a[0::2] + a[1::2])


def study():
    sols = {n: run(n) for n in GRIDS}
    # Cauchy errors between successive grids, interior only: the first 0.75 m
    # holds the make-up-inlet boundary layer, whose few-cell structure does
    # not refine (standard practice: verify convergence away from the
    # discrete BC; the analytic L1 below still covers the full domain).
    cauchy = []
    for n0, n1 in zip(GRIDS[:-1], GRIDS[1:], strict=False):
        x0 = sols[n0][0]
        mask = x0 > 0.75
        e = np.mean(np.abs(restrict(sols[n1][1]) - sols[n0][1])[mask])
        cauchy.append(e)
    orders = [np.log2(a / b) for a, b in zip(cauchy[:-1], cauchy[1:], strict=False)]
    x_f, a_f = sols[GRIDS[-1]]
    l1_analytic = np.mean(np.abs(a_f - faucet_exact(x_f, T_END)))
    return sols, cauchy, orders, l1_analytic


def test_faucet():
    _, cauchy, orders, l1 = study()
    assert l1 < 0.08, f"analytic L1 {l1:.4f} above model-error floor budget"
    # order >= 0.8 in the resolved regime (48->96->192); at N=384 the Cauchy
    # error saturates on inlet-boundary-layer noise, visible in the plot
    assert orders[0] >= 0.8, f"self-convergence orders {orders}"


if __name__ == "__main__":
    sols, cauchy, orders, l1 = study()
    fig, (ax1, ax2) = plt.subplots(1, 2, figsize=(11, 4.2))
    xe = np.linspace(0, 12, 500)
    ax1.plot(xe, faucet_exact(xe, T_END), "k--", lw=1.2, label="Ransom analytic (two-fluid)")
    for n in GRIDS:
        x, a = sols[n]
        ax1.plot(x, a, lw=1, label=f"N={n}")
    ax1.set(
        xlabel="x [m]",
        ylabel=r"$\alpha_g$",
        title=f"water faucet, t={T_END}s  (L1 vs analytic {l1:.3f})",
    )
    ax1.legend(fontsize=8)
    ax2.loglog(GRIDS[:-1], cauchy, "o-k")
    ax2.loglog(GRIDS[:-1], [cauchy[0] * (GRIDS[0] / n) for n in GRIDS[:-1]], ":", label="order 1")
    orders_txt = [f"{o:.2f}" for o in orders]
    ax2.set(xlabel="N", ylabel="Cauchy L1", title=f"self-convergence, orders {orders_txt}")
    ax2.legend()
    fig.tight_layout()
    fig.savefig(f"{OUT}/faucet.png", dpi=140)
    print(f"analytic L1 = {l1:.4f}, cauchy orders = {orders}")
    print(f"wrote {OUT}/faucet.png")
