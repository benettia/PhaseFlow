"""Web boot smoke: serve web/, load in headless chromium, require the sim to
boot, run, and log no console errors. Playwright is the one browser dev-dep.

Run: uv run analysis/smoke_web.py
"""

import http.server
import os
import threading

from playwright.sync_api import sync_playwright

ROOT = os.path.join(os.path.dirname(__file__), "..", "web")


def serve(port: int):
    def handler(*a, **kw):
        return http.server.SimpleHTTPRequestHandler(*a, directory=ROOT, **kw)

    httpd = http.server.ThreadingHTTPServer(("127.0.0.1", port), handler)
    threading.Thread(target=httpd.serve_forever, daemon=True).start()
    return httpd


def run_smoke() -> list[str]:
    assert os.path.exists(os.path.join(ROOT, "pkg", "phase_flow_wasm.js")), (
        "web/pkg is missing (it is gitignored) — build it first: "
        "wasm-pack build crates/wasm --target web --release --out-dir ../../web/pkg"
    )
    httpd = serve(0)
    port = httpd.server_address[1]
    errors: list[str] = []
    with sync_playwright() as pw:
        browser = pw.chromium.launch()
        page = browser.new_page(accept_downloads=True, viewport={"width": 1500, "height": 940})
        page.on("console", lambda m: errors.append(m.text) if m.type == "error" else None)
        page.on("pageerror", lambda e: errors.append(str(e)))
        page.goto(f"http://127.0.0.1:{port}/", wait_until="networkidle")
        page.wait_for_function("window.PHASEFLOW_READY === true", timeout=15000)
        t0 = float(page.text_content("#t-now"))
        page.wait_for_timeout(1500)
        t1 = float(page.text_content("#t-now"))
        assert t1 > t0, f"sim time did not advance: {t0} -> {t1}"

        # timeline: scrub back, then resume and confirm the solver rewound
        scrub = page.locator("#scrub")
        last = int(scrub.get_attribute("max"))
        assert last > 4, f"history too short to scrub: {last + 1} frames"
        scrub.fill(str(last // 3))
        scrub.dispatch_event("input")
        page.wait_for_timeout(300)
        t_rewound = float(page.text_content("#t-now"))
        assert t_rewound < t1, f"scrubbing did not rewind: {t_rewound} vs {t1}"
        assert "reviewing" in (page.get_attribute("body", "class") or "")
        page.locator("#run").click()  # resume => roll the solver back
        page.wait_for_timeout(1200)
        t_resumed = float(page.text_content("#t-now"))
        assert t_rewound < t_resumed < t1, (
            f"resume did not continue from the rewound point: {t_rewound} -> {t_resumed} (was {t1})"
        )
        # every analysis tab must draw something (a blank canvas means the
        # view threw and the error was swallowed by requestAnimationFrame)
        for view in ("trends", "map", "profiles"):
            page.locator(f"#tabs button[data-view={view}]").click()
            page.wait_for_timeout(400)
            assert page.evaluate(
                "() => { const c = document.querySelector('.view.on canvas');"
                " return c.width > 50 && c.height > 50; }"
            ), f"{view} canvas has no size"

        # click through every preset and let each run briefly
        for btn in page.locator("button.preset").all():
            btn.click()
            page.wait_for_timeout(900)

        # design report must carry real numbers, not placeholders
        page.locator("button.preset", has_text="severe slugging").first.click()
        page.wait_for_timeout(1200)
        for field in ("ro-liq", "ro-holdup", "ro-ero", "ro-amin"):
            txt = page.text_content(f"#{field}")
            assert txt and txt != "—", f"report field {field} never filled: {txt!r}"

        # switching fluid rebuilds the sim with the new properties
        page.select_option("#fluid", "gas-oil")
        page.wait_for_timeout(700)
        assert "850" in page.text_content("#fluid-props"), "fluid properties did not update"

        # the energy equation can be switched on live, and adds its panel
        page.check("#thermal")
        page.wait_for_timeout(900)
        assert page.text_content("#ro-trange").strip() not in ("", "—"), "no temperature readout"

        # CSV export produces a real download
        with page.expect_download() as dl:
            page.locator("#export").click()
        path = dl.value.path()
        with open(path) as fh:
            head, first = fh.readline(), fh.readline()
        assert head.startswith("x_m,elevation_m"), f"unexpected CSV header {head!r}"
        assert len(first.split(",")) == 10, f"unexpected CSV row {first!r}"

        banner = page.evaluate("document.getElementById('banner').style.display")
        assert banner in ("", "none"), "error banner is showing"
        page.screenshot(path=os.path.join(os.path.dirname(__file__), "out", "web.png"))
        browser.close()
    httpd.shutdown()
    return errors


def test_web_boots():
    errors = run_smoke()
    assert not errors, f"console errors: {errors}"


if __name__ == "__main__":
    errs = run_smoke()
    print("console errors:", errs or "none")
    print("screenshot: analysis/out/web.png")
