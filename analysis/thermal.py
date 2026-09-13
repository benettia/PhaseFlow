"""Energy-equation verification: three thermal processes, each against a
closed-form result rather than against itself.

1. Wall heat transfer — a closed, stagnant, gravity-free pipe has no flux
   gradient at all, so the energy equation collapses to
   `rho_m c_m dT/dt = 4U/D (T_amb - T)`: an exponential whose time constant
   is fixed by the initial state (the masses cannot change).
2. Frictional heating — in steady single-phase liquid flow the temperature
   rise along the line is exactly the friction pressure drop divided by
   `rho c_p`. Every joule the pump spends on friction ends up in the water.
3. Expansion cooling — an insulated gas line blowing down cools along the
   isentrope `T/T0 = (p/p0)^((g-1)/g)`, to the extent a pipe behaves like a
   uniform vessel (gradients and mixing behind the expansion wave leave it
   warmer than isentropic, never colder).

Run: uv run analysis/thermal.py  (writes analysis/out/thermal.png)
"""

import matplotlib

matplotlib.use("Agg")
import matplotlib.pyplot as plt
import numpy as np
import phase_flow as pf
from common import OUT, make_sim

AW = pf.fluid_properties("air-water")
GO = pf.fluid_properties("gas-oil")


# ---------- 1. lumped wall heat transfer ----------

T0_LUMP, TAMB_LUMP, U_LUMP, D_LUMP = 350.0, 290.0, 1000.0, 0.1


def lumped_scenario() -> dict:
    return {
        "segments": [
            {
                "length": 4,
                "angle": 0,
                "diameter": D_LUMP,
                "cells": 20,
                "init": {"alpha_g": 0.999, "p": 5e5, "v": 0, "t": T0_LUMP},
            }
        ],
        "init": {"alpha_g": 0.999, "p": 5e5, "v": 0, "t": T0_LUMP},
        "inlet": {"wg": 0, "wl": 0},
        "outlet": {"p": 5e5, "choke": 0},
        "options": {
            "thermal": True,
            "wall_friction": False,
            "g": 0,
            "u_wall": U_LUMP,
            "t_ambient": TAMB_LUMP,
        },
    }


def run_lumped():
    sim = make_sim(lumped_scenario())
    mg, ml = sim.rho_g[0] * sim.alpha_g[0], sim.rho_l[0] * (1 - sim.alpha_g[0])
    cv_gas = AW["gas_cp"] - AW["r_gas"]
    tau = (mg * cv_gas + ml * AW["liq_cp"]) * D_LUMP / (4 * U_LUMP)
    ts, got = [], []
    while sim.time < 6 * tau:
        sim.run(tau / 40)
        ts.append(sim.time)
        got.append(float(sim.temperature[10]))
    ts, got = np.array(ts), np.array(got)
    exact = TAMB_LUMP + (T0_LUMP - TAMB_LUMP) * np.exp(-ts / tau)
    return ts, got, exact, tau


# ---------- 2. frictional heating ----------

D_FRIC, L_FRIC, V_FRIC = 0.05, 100.0, 6.0


def friction_scenario() -> dict:
    area = 0.25 * np.pi * D_FRIC**2
    rho = AW["liq_rho"]
    return {
        "segments": [
            {
                "length": L_FRIC,
                "angle": 0,
                "diameter": D_FRIC,
                "cells": 100,
                "init": {"alpha_g": 1e-5, "p": 1e6, "v": V_FRIC},
            }
        ],
        "init": {"alpha_g": 1e-5, "p": 1e6, "v": V_FRIC},
        "inlet": {"wg": 0, "wl": rho * V_FRIC * area},
        "outlet": {"p": 1e6, "choke": 1.0, "cv": 200},
        "options": {"thermal": True, "g": 0, "u_wall": 0.0},
    }


def run_friction():
    sim = make_sim(friction_scenario())
    # ~7 residence times: the thermal field is the slow one. Chunked because
    # a water line runs at the liquid acoustic CFL (dt ~ 3e-4 s), and one
    # advance() call is capped at 200k substeps.
    for _ in range(12):
        sim.run(10.0)
    r = sim.report()
    t = sim.temperature
    dt_sim = float(t[-1] - t[0])
    # every joule of friction lands in the liquid: dT = dp_friction / (rho cp)
    dt_exact = r["dp_friction"] / (AW["liq_rho"] * AW["liq_cp"])
    return sim.x.copy(), t.copy(), sim.p.copy(), dt_sim, dt_exact, r


# ---------- 3. blowdown expansion cooling ----------

P0_BD, T0_BD = 8.0e6, 310.0


def blowdown_scenario() -> dict:
    return {
        "segments": [
            {
                "length": 200,
                "angle": 0,
                "diameter": 0.15,
                "cells": 120,
                "init": {"alpha_g": 1 - 1e-6, "p": P0_BD, "v": 0, "t": T0_BD},
            }
        ],
        "init": {"alpha_g": 1 - 1e-6, "p": P0_BD, "v": 0, "t": T0_BD},
        "inlet": {"wg": 0, "wl": 0},
        "outlet": {"p": 1e5, "choke": 0.1, "cv": 3},
        "options": {"thermal": True, "wall_friction": False, "g": 0, "u_wall": 0.0},
        "fluid": {"preset": "gas-oil"},
    }


def run_blowdown():
    sim = make_sim(blowdown_scenario())
    gamma = GO["gamma"]
    ts, pm, tm = [], [], []
    while sim.time < 40.0:
        sim.run(0.25)
        w = sim.rho_g * sim.alpha_g * sim.area * sim.dx
        ts.append(sim.time)
        pm.append(float(np.sum(w * sim.p) / np.sum(w)))
        tm.append(float(np.sum(w * sim.temperature) / np.sum(w)))
    ts, pm, tm = np.array(ts), np.array(pm), np.array(tm)
    isen = T0_BD * (pm / P0_BD) ** ((gamma - 1) / gamma)
    return ts, pm, tm, isen


# ---------- assertions ----------


def test_wall_heat_transfer_is_exponential():
    ts, got, exact, tau = run_lumped()
    err = np.max(np.abs(got - exact)) / (T0_LUMP - TAMB_LUMP)
    assert err < 0.005, f"lumped relaxation off by {100 * err:.2f} % of the span (tau={tau:.3f}s)"


def test_friction_heats_the_liquid():
    _, _, _, dt_sim, dt_exact, r = run_friction()
    assert dt_exact > 0.05, f"case too weak to test: dT_exact {dt_exact:.4f} K"
    assert abs(r["dp_accel"]) < 0.03 * r["dp_friction"], "not steady enough to compare"
    err = abs(dt_sim - dt_exact) / dt_exact
    assert err < 0.06, f"friction heating {dt_sim:.4f} K vs dp/(rho cp) {dt_exact:.4f} K"


def test_blowdown_tracks_isentropic():
    ts, pm, tm, isen = run_blowdown()
    late = ts > 5.0
    assert pm[-1] < 0.1 * P0_BD, f"line did not blow down: {pm[-1]:.2e} Pa"
    # never colder than isentropic: irreversibility can only add heat
    assert np.all(tm[late] > isen[late] - 2.0), "cooled past the isentrope"
    # and recognisably close to it, not merely bounded by it
    spread = np.max((tm[late] - isen[late]) / (T0_BD - isen[late]))
    assert spread < 0.3, f"drifted {100 * spread:.0f} % of the way back to isothermal"


if __name__ == "__main__":
    fig, axes = plt.subplots(1, 3, figsize=(13.5, 4.2))

    ts, got, exact, tau = run_lumped()
    axes[0].plot(ts / tau, got - 273.15, color="#1c1b18", lw=1.4, label="solver")
    axes[0].plot(ts / tau, exact - 273.15, "--", color="#a4442c", lw=1.1, label="lumped exact")
    err = np.max(np.abs(got - exact)) / (T0_LUMP - TAMB_LUMP)
    axes[0].set(
        xlabel=r"$t/\tau$",
        ylabel="T [°C]",
        title=f"wall heat transfer\nτ = {tau:.3f} s, max error {100 * err:.2f} % of span",
    )
    axes[0].legend(fontsize=8)

    x, t, p, dt_sim, dt_exact, r = run_friction()
    ax2 = axes[1]
    ax2.plot(x, t - t[0], color="#1c1b18", lw=1.4, label="solver ΔT(x)")
    ax2.plot(
        x,
        (p[0] - p) / (AW["liq_rho"] * AW["liq_cp"]),
        "--",
        color="#a4442c",
        lw=1.1,
        label=r"$\Delta p / (\rho c_p)$",
    )
    ax2.set(
        xlabel="x [m]",
        ylabel="ΔT [K]",
        title=f"frictional heating\n{dt_sim:.4f} K vs {dt_exact:.4f} K exact "
        f"({100 * abs(dt_sim - dt_exact) / dt_exact:.1f} %)",
    )
    ax2.legend(fontsize=8)

    ts, pm, tm, isen = run_blowdown()
    ax3 = axes[2]
    ax3.plot(pm / 1e5, tm - 273.15, color="#1c1b18", lw=1.4, label="solver (mass-averaged)")
    ax3.plot(pm / 1e5, isen - 273.15, "--", color="#a4442c", lw=1.1, label="isentropic")
    ax3.axhline(T0_BD - 273.15, color="#a8a091", lw=0.8, ls=":", label="isothermal")
    ax3.invert_xaxis()
    ax3.set(xlabel="mean p [bar]", ylabel="mean T [°C]", title="blowdown expansion cooling")
    ax3.legend(fontsize=8)

    fig.tight_layout()
    fig.savefig(f"{OUT}/thermal.png", dpi=140)
    print(f"lumped max error {100 * err:.3f} % of span")
    print(f"friction heating {dt_sim:.4f} K vs exact {dt_exact:.4f} K")
    print(
        f"blowdown: {pm[0] / 1e5:.0f} -> {pm[-1] / 1e5:.2f} bar, T {tm[0] - 273.15:.1f} -> "
        f"{tm[-1] - 273.15:.1f} °C (isentropic {isen[-1] - 273.15:.1f} °C)"
    )
    print(f"wrote {OUT}/thermal.png")
