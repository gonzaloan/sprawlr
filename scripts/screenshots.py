"""Regenerate the README screenshots and the tour GIF in docs/images from the web demo.

    cd app && npx vite build && npx vite preview --port 4173 &
    python3 scripts/screenshots.py [--tour] [http://localhost:4173/]

--tour also records docs/images/tour.gif (needs ffmpeg). Each recording differs by a few bytes
and weighs about 2 MB in the history, so re-record it only when the tour itself looks different.
Needs Playwright for Python (pip install playwright && playwright install chromium).
"""

import subprocess
import sys
import tempfile
from pathlib import Path

from playwright.sync_api import Browser, Page, sync_playwright

ARGS = [a for a in sys.argv[1:] if not a.startswith("--")]
TOUR = "--tour" in sys.argv[1:]
BASE = ARGS[0] if ARGS else "http://localhost:4173/"
OUT = Path(__file__).resolve().parent.parent / "docs" / "images"


def open_demo(page: Page, query: str = "") -> None:
    page.goto(BASE + query)
    page.wait_for_timeout(1500)


def side(page: Page, name: str) -> None:
    page.locator(".side__item", has_text=name).first.click()
    page.wait_for_timeout(1200)


def node(page: Page, name: str) -> None:
    page.locator(".react-flow__node", has_text=name).first.click()
    page.wait_for_timeout(1000)


def set_slider(page: Page, value: str) -> None:
    """Moves the traffic slider the way React sees a user drag it."""
    page.locator(".whatif input[type=range]").evaluate(
        "(el, v) => { const set = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set;"
        " set.call(el, v); el.dispatchEvent(new Event('input', { bubbles: true })); }",
        value,
    )


def shoot(page: Page, name: str) -> None:
    page.screenshot(path=str(OUT / name))
    print("wrote", OUT / name)


def tour(browser: Browser) -> None:
    """A 15-second walk through the demo: briefing, a flow under load, why it fails."""
    with tempfile.TemporaryDirectory() as tmp:
        ctx = browser.new_context(viewport={"width": 1280, "height": 800}, record_video_dir=tmp, record_video_size={"width": 1280, "height": 800})
        page = ctx.new_page()
        open_demo(page)
        page.wait_for_timeout(1800)
        side(page, "Web requests to checkout")
        page.wait_for_timeout(1200)
        page.get_by_role("radio", name="Capacity", exact=True).click()
        for value in ["1.2", "1.4", "1.6", "1.8", "2"]:
            set_slider(page, value)
            page.wait_for_timeout(450)
        page.wait_for_timeout(1200)
        page.get_by_role("radio", name="Health", exact=True).click()
        node(page, "InvoiceProcessor")
        page.wait_for_timeout(1200)
        page.locator(".details h3", has_text="Errors in logs").evaluate("(el) => el.scrollIntoView({ block: 'start', behavior: 'smooth' })")
        page.wait_for_timeout(2500)
        video = Path(page.video.path())
        ctx.close()
        gif = OUT / "tour.gif"
        palette = Path(tmp) / "palette.png"
        scale = "fps=8,scale=800:-1:flags=lanczos"
        ffmpeg = ["ffmpeg", "-y", "-loglevel", "error", "-ss", "1", "-i", str(video)]
        subprocess.run([*ffmpeg, "-vf", f"{scale},palettegen=max_colors=96:stats_mode=diff", str(palette)], check=True)
        subprocess.run(
            [*ffmpeg, "-i", str(palette), "-lavfi", f"{scale}[x];[x][1:v]paletteuse=dither=bayer:bayer_scale=4:diff_mode=rectangle", str(gif)],
            check=True,
        )
        print("wrote", gif)


def main() -> None:
    with sync_playwright() as p:
        browser = p.chromium.launch()
        page = browser.new_page(viewport={"width": 1440, "height": 900})

        open_demo(page)
        shoot(page, "overview.png")

        side(page, "Web requests to checkout")
        page.get_by_role("radio", name="Capacity", exact=True).click()
        set_slider(page, "1.5")
        page.wait_for_timeout(800)
        shoot(page, "flow-whatif.png")

        page.get_by_role("radio", name="Health", exact=True).click()
        node(page, "InvoiceProcessor")
        page.locator(".details h3", has_text="Errors in logs").evaluate("(el) => el.scrollIntoView({ block: 'start' })")
        page.wait_for_timeout(500)
        shoot(page, "error-logs.png")

        open_demo(page)
        side(page, "Files uploaded to reports-uploads")
        node(page, "reports-worker")
        shoot(page, "iam-mapped.png")

        open_demo(page)
        side(page, "Probably not needed")
        shoot(page, "cleanup.png")

        tray = browser.new_page(viewport={"width": 360, "height": 480}, device_scale_factor=2)
        open_demo(tray, "?view=tray")
        shoot(tray, "tray-popover.png")

        if TOUR:
            tour(browser)
        browser.close()


if __name__ == "__main__":
    main()
