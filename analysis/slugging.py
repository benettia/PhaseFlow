"""Severe slugging cycle analysis — the four-stage limit cycle must emerge
from the equations, unscripted, with a stable period (±10 % consecutive).

Run: uv run analysis/slugging.py  (writes analysis/out/slugging.png)
"""

import matplotlib

matplotlib.use("Agg")
import matplotlib.pyplot as plt
import numpy as np
from common import OUT, find_peaks, make_sim, slugging_scenario

T_END = 900.0


def run():
    sim = make_sim(slugging_scenario())
    base = 150  # first riser cell
    ts, ps, hold = [], [], []
    while sim.time < T_END:
        sim.run(0.5)
        ts.append(sim.time)
        ps.append(sim.p[base])
        a = sim.alpha_g
        hold.append(1 - a[150:].mean())  # riser liquid inventory
    return np.array(ts), np.array(ps), np.array(hold)


def periods_of(ts, ps):
    # skip the startup third, smooth lightly, find blowdown peaks
    i0 = len(ts) // 3
    k = np.ones(5) / 5
    sm = np.convolve(ps[i0:], k, mode="same")
    peaks = find_peaks(ts[i0:], sm, min_sep=60.0, frac=0.55)
    # a peak near the end of the record belongs to a truncated cycle
    peaks = peaks[peaks < ts[-1] - 30.0]
    return np.diff(peaks), peaks


def test_severe_slugging_period():
    ts, ps, _ = run()
    per, _ = periods_of(ts, ps)
    assert len(per) >= 2, f"need >=3 peaks for 2 periods, got {len(per) + 1}"
    ratio = per[-1] / per[-2]
    assert abs(ratio - 1) < 0.10, f"consecutive periods {per[-2]:.1f}, {per[-1]:.1f} s"
    swing = ps.max() - ps.min()
    assert swing > 5e4, f"riser-base swing only {swing / 1e3:.1f} kPa"


if __name__ == "__main__":
    ts, ps, hold = run()
    per, peaks = periods_of(ts, ps)
    fig, (ax1, ax2) = plt.subplots(2, 1, figsize=(10, 6), sharex=True)
    ax1.plot(ts, ps / 1e3, "k-", lw=0.9)
    for pk in peaks:
        ax1.axvline(pk, color="#a4442c", lw=0.6, ls=":")
    ax1.set(
        ylabel="riser-base p [kPa]",
        title=f"severe slugging limit cycle — periods {np.round(per, 1)} s",
    )
    ax2.plot(ts, hold, color="#1f4e79", lw=0.9)
    ax2.set(xlabel="t [s]", ylabel="riser liquid holdup")
    fig.tight_layout()
    fig.savefig(f"{OUT}/slugging.png", dpi=140)
    print(f"periods: {np.round(per, 1)} s, swing {(ps.max() - ps.min()) / 1e3:.0f} kPa")
    print(f"wrote {OUT}/slugging.png")
