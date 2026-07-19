"""Gas kick: gas injected at the bottom of a liquid-filled vertical well must
migrate upward, accelerate as it expands into falling pressure, and unload
the liquid column — the classic well-control transient.

Run: uv run analysis/gas_kick.py  (writes analysis/out/gas_kick.png)
"""

import matplotlib

matplotlib.use("Agg")
import matplotlib.pyplot as plt
import numpy as np
from common import OUT, make_sim

AREA = 0.25 * np.pi * 0.12**2


def kick_scenario() -> dict:
    return {
        "segments": [
            {
                "length": 30,
                "angle": 90,
                "diameter": 0.12,
                "cells": 100,
                "init": {"alpha_g": 0.02, "p": 3e5, "v": 0},
            }
        ],
        "init": {"alpha_g": 0.02, "p": 3e5, "v": 0},
        "inlet": {"wg": 0.03, "wl": 0.2},
        "outlet": {"p": 1e5, "choke": 1.0, "cv": 0.8},
        "options": {"hydrostatic_init": True},
    }


def front_position(x: np.ndarray, a: np.ndarray, thresh=0.15) -> float:
    """Leading edge of the bottom-connected gas region (ignores the small
    outlet boundary blip in the top cells)."""
    below = np.where(a < thresh)[0]
    return float(x[below[0]]) if len(below) else float(x[-1])


def run():
    sim = make_sim(kick_scenario())
    ml0 = sim.mass_totals()[1]
    ts, fronts, liq, rho_gas_mean, pbot = [], [], [], [], []
    profiles = []
    while sim.time < 60.0:
        sim.run(0.5)
        a = sim.alpha_g
        mg, ml = sim.mass_totals()
        vgas = float(np.sum(a * sim.dx)) * AREA
        ts.append(sim.time)
        fronts.append(front_position(sim.x, a))
        liq.append(ml / ml0)
        rho_gas_mean.append(mg / vgas)
        pbot.append(sim.p[0])
        profiles.append(a.copy())
    return (
        np.array(ts),
        np.array(fronts),
        np.array(liq),
        np.array(rho_gas_mean),
        np.array(pbot),
        np.array(profiles),
        sim.x,
    )


def test_gas_kick():
    ts, fronts, liq, rho, _, _, _ = run()

    def at(t):
        return int(np.searchsorted(ts, t))

    # migration: front rises through the column at a finite, plausible rate
    assert 4.0 < fronts[at(6)] < 12.0, f"front at t=6: {fronts[at(6)]:.1f} m"
    assert 18.0 < fronts[at(14)] < 29.0, f"front at t=14: {fronts[at(14)]:.1f} m"
    # expansion accelerates the front as it rises into lower pressure
    v_early = (fronts[at(6)] - fronts[at(2)]) / 4.0
    v_late = (fronts[at(14)] - fronts[at(10)]) / 4.0
    assert v_late > 1.2 * v_early, f"front speed {v_early:.2f} -> {v_late:.2f} m/s"
    # expansion: mean gas density drops hard during the migration
    assert rho[at(18)] < 0.6 * rho[at(4)], f"rho_gas {rho[at(4)]:.2f} -> {rho[at(18)]:.2f}"
    # unloading: the column is evacuated
    assert liq[at(60) - 1] < 0.1, f"liquid fraction at t=60: {liq[at(60) - 1]:.3f}"


if __name__ == "__main__":
    ts, fronts, liq, rho, pbot, profiles, x = run()
    fig, (ax1, ax2) = plt.subplots(1, 2, figsize=(11, 4.4))
    im = ax1.imshow(
        profiles.T,
        origin="lower",
        aspect="auto",
        cmap="YlGnBu_r",
        extent=(ts[0], ts[-1], x[0], x[-1]),
        vmin=0,
        vmax=1,
    )
    ax1.plot(ts, fronts, "w--", lw=1, label="gas front")
    ax1.set(xlabel="t [s]", ylabel="height [m]", title="void fraction — kick migration")
    ax1.legend(loc="lower right", fontsize=8)
    fig.colorbar(im, ax=ax1, label=r"$\alpha_g$")
    ax2.plot(ts, liq, color="#1f4e79", label="liquid inventory / initial")
    ax2.plot(ts, rho / rho[0], color="#d9a441", label="mean gas density / initial")
    ax2.plot(ts, pbot / pbot[0], color="#1c1b18", ls=":", label="bottomhole p / initial")
    ax2.set(xlabel="t [s]", title="expansion and unloading")
    ax2.legend(fontsize=8)
    fig.tight_layout()
    fig.savefig(f"{OUT}/gas_kick.png", dpi=140)
    print(
        f"front speeds: early {(fronts[12] - fronts[4]) / 4:.2f}, late "
        f"{(fronts[28] - fronts[20]) / 4:.2f} m/s; final liquid {liq[-1]:.3f}"
    )
    print(f"wrote {OUT}/gas_kick.png")
