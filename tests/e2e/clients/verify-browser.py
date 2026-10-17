"""Open a verification link at a phone's width and a desktop's: `look`
checks the page offers a Confirm button that fits and is big enough to
tap, with the code field showing only without JavaScript or for a link
cut short; `confirm` presses it. Saves screenshots in /tmp/screens."""

import os
import sys

from playwright.sync_api import expect

from browser import browsing

WIDTHS = {"phone": 375, "desktop": 1280}
SCREENS = "/tmp/screens"


def fits(page):
    return page.evaluate("document.documentElement.scrollWidth <= window.innerWidth")


def confirm_button(page):
    return page.get_by_role("button", name="Confirm my email")


def visit(page, url):
    """Loads `url` afresh: from a URL it differs from only in the fragment,
    a plain goto would keep the page and not run its script again."""
    page.goto("about:blank")
    page.goto(url)


def look(page, link):
    for name, width in WIDTHS.items():
        page.set_viewport_size({"width": width, "height": 800})
        visit(page, link)
        expect(page.locator("#code")).to_be_hidden()
        assert confirm_button(page).bounding_box()["height"] >= 44, name
        assert fits(page), name
        page.screenshot(path=f"{SCREENS}/verify-{name}.png", full_page=True)

        visit(page, link[:-1])
        expect(page.locator("#code-hint")).to_contain_text("This link looks cut short.")
        expect(page.locator("#code")).to_be_visible()

    without_js = page.context.browser.new_page(ignore_https_errors=True, java_script_enabled=False)
    for name, width in WIDTHS.items():
        without_js.set_viewport_size({"width": width, "height": 800})
        without_js.goto(link)
        code = without_js.locator("#code")
        expect(code).to_be_visible()
        assert code.evaluate("input => parseFloat(getComputedStyle(input).fontSize)") >= 16, name
        assert code.bounding_box()["height"] >= 44, name
        assert fits(without_js), name
        without_js.screenshot(path=f"{SCREENS}/verify-no-js-{name}.png", full_page=True)
    print("the page fits at both widths and verifies nothing by loading")


def confirm(page, link):
    page.set_viewport_size({"width": WIDTHS["phone"], "height": 800})
    page.goto(link)
    confirm_button(page).click()
    expect(page.locator("h1")).to_have_text("Your email is confirmed.")
    for name, width in WIDTHS.items():
        page.set_viewport_size({"width": width, "height": 800})
        assert page.get_by_role("link", name="Go to your account").bounding_box()["height"] >= 44, name
        assert fits(page), name
        page.screenshot(path=f"{SCREENS}/verified-{name}.png", full_page=True)
    print("confirmed from the link")


def main():
    action, link = sys.argv[1:]
    os.makedirs(SCREENS, exist_ok=True)
    with browsing() as page:
        {"look": look, "confirm": confirm}[action](page, link)


if __name__ == "__main__":
    main()
