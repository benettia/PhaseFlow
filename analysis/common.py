"""Shared scenario builders + helpers for the verification studies."""

import json
import os

import numpy as np
import phase_flow as pf

OUT = os.path.join(os.path.dirname(__file__), "out")
os.makedirs(OUT, exist_ok=True)

A_G = 316.0


def make_sim(scenario: dict) -> pf.Sim:
    return pf.Sim.from_json(json.dumps(scenario))


def faucet_scenario(cells: int, fixed_dt: float | None = None) -> dict:
    area = 0.25 * np.pi
    return {
        "segments": [{"length": 12, "angle": -90, "diameter": 1.0, "cells": cells}],
        "init": {"alpha_g": 0.2, "p": 1e5, "v": 10},
        "inlet": {"wg": 0.0, "wl": 0.8 * 1000 * 10 * area, "p_anchor": 1e5, "makeup_alpha": 0.2},
        "outlet": {"p": 1e5, "choke": 1.0, "cv": 20},
        "options": {"wall_friction": False, "fixed_dt": fixed_dt},
    }


def faucet_exact(x: np.ndarray, t: float, v0=10.0, al0=0.8, g=9.81) -> np.ndarray:
    """Ransom's analytic transient void profile."""
    front = v0 * t + 0.5 * g * t * t
    a = 1.0 - al0 * v0 / np.sqrt(v0 * v0 + 2 * g * x)
    return np.where(x <= front, a, 1.0 - al0)


def shock_tube_scenario(n: int, pl=4e5, pr=1e5) -> dict:
    seg = {"length": 50, "angle": 0, "diameter": 0.1}
    return {
        "segments": [
            {**seg, "cells": n // 2, "init": {"alpha_g": 1 - 1e-6, "p": pl, "v": 0}},
            {**seg, "cells": n // 2, "init": {"alpha_g": 1 - 1e-6, "p": pr, "v": 0}},
        ],
        "init": {"alpha_g": 0.5, "p": 1e5, "v": 0},
        "inlet": {"wg": 0, "wl": 0},
        "outlet": {"p": 1e5, "choke": 0},
        "options": {"wall_friction": False, "g": 0},
    }


def isothermal_riemann(pl, pr, a=A_G):
    """Exact middle state + shock speed for both-at-rest isothermal Riemann."""
    rl, rr = pl / a**2, pr / a**2
    lo, hi = rr, rl
    for _ in range(200):
        r = 0.5 * (lo + hi)
        u_rar = -a * np.log(r / rl)
        u_shk = (r - rr) * a / np.sqrt(r * rr)
        if u_rar > u_shk:
            lo = r
        else:
            hi = r
    r = 0.5 * (lo + hi)
    u = (r - rr) * a / np.sqrt(r * rr)
    s = r * u / (r - rr)
    return r, u, s


def riemann_profile(x, t, pl, pr, a=A_G, x0=50.0):
    """Full exact solution (density) sampled at positions x, time t."""
    rl, rr = pl / a**2, pr / a**2
    rstar, ustar, s = isothermal_riemann(pl, pr, a)
    xi = (np.asarray(x) - x0) / t
    rho = np.empty_like(xi)
    head, tail = -a, ustar - a
    rho[xi <= head] = rl
    fan = (xi > head) & (xi <= tail)
    u_fan = xi[fan] + a
    rho[fan] = rl * np.exp(-u_fan / a)
    rho[(xi > tail) & (xi <= s)] = rstar
    rho[xi > s] = rr
    return rho * a**2  # pressure


def slugging_scenario() -> dict:
    """Same as the web severe-slugging preset."""
    return {
        "segments": [
            {
                "length": 80,
                "angle": -4,
                "diameter": 0.08,
                "cells": 150,
                "init": {"alpha_g": 0.6, "p": 2e5, "v": 0},
            },
            {
                "length": 12,
                "angle": 90,
                "diameter": 0.08,
                "cells": 45,
                "init": {"alpha_g": 0.03, "p": 2e5, "v": 0},
            },
        ],
        "init": {"alpha_g": 0.5, "p": 2e5, "v": 0},
        "inlet": {"wg": 0.003, "wl": 2.0},
        "outlet": {"p": 1e5, "choke": 1.0, "cv": 0.6},
        "options": {"hydrostatic_init": True, "regime_feedback": True},
    }


def valve_slam_scenario() -> dict:
    return {
        "segments": [{"length": 100, "angle": 0, "diameter": 0.1, "cells": 200}],
        "init": {"alpha_g": 0.1, "p": 2e5, "v": 1},
        "inlet": {"wg": 1.6e-3, "wl": 7.07},
        "outlet": {"p": 1.9e5, "choke": 1.0, "cv": 1.5},
        "options": {},
    }


def find_peaks(t: np.ndarray, y: np.ndarray, min_sep: float, frac=0.5):
    """Local maxima above min + frac*range, separated by min_sep seconds."""
    lo, hi = y.min(), y.max()
    thresh = lo + frac * (hi - lo)
    peaks = []
    for i in range(1, len(y) - 1):
        if y[i] >= thresh and y[i] >= y[i - 1] and y[i] > y[i + 1]:
            if not peaks or t[i] - peaks[-1] > min_sep:
                peaks.append(t[i])
    return np.array(peaks)
