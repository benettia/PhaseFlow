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
        page = browser.new_page()
        page.on("console", lambda m: errors.append(m.text) if m.type == "error" else None)
        page.on("pageerror", lambda e: errors.append(str(e)))
        page.goto(f"http://127.0.0.1:{port}/", wait_until="networkidle")
        page.wait_for_function("window.PHASEFLOW_READY === true", timeout=15000)
        t0 = page.evaluate("document.getElementById('status').textContent")
        page.wait_for_timeout(1500)
        t1 = page.evaluate("document.getElementById('status').textContent")
        assert t0 != t1, f"sim time did not advance: {t0!r}"
        # click through every preset and let each run briefly
        for btn in page.locator("button.preset").all():
            btn.click()
            page.wait_for_timeout(900)
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
