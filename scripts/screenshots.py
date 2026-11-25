"""Regenerate the README screenshots in docs/images from the web demo.

    cd app && npx vite build && npx vite preview --port 4173 &
    python3 scripts/screenshots.py [http://localhost:4173/]

Needs Playwright for Python (pip install playwright && playwright install chromium).
"""

import sys
from pathlib import Path

from playwright.sync_api import Page, sync_playwright

BASE = sys.argv[1] if len(sys.argv) > 1 else "http://localhost:4173/"
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


def shoot(page: Page, name: str) -> None:
    page.screenshot(path=str(OUT / name))
    print("wrote", OUT / name)


def main() -> None:
    with sync_playwright() as p:
        browser = p.chromium.launch()
        page = browser.new_page(viewport={"width": 1440, "height": 900})

        open_demo(page)
        shoot(page, "overview.png")

        side(page, "Web requests to checkout")
        page.get_by_role("radio", name="Load", exact=True).click()
        slider = page.locator(".whatif input[type=range]")
        slider.evaluate(
            "(el) => { const set = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set;"
            " set.call(el, '1.5'); el.dispatchEvent(new Event('input', { bubbles: true })); }"
        )
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

        browser.close()


if __name__ == "__main__":
    main()
