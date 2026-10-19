"""Open a reset link at a phone's width and a desktop's: `reset` checks the
page fits, hides the code field the link fills in, and has a button big
enough to tap, then sets the password and lands on the page that says to
sign in. Saves screenshots in /tmp/screens."""

import os
import sys

from playwright.sync_api import expect

from browser import SCREENS, WIDTHS, browsing, fits


def reset(page, link, password):
    button = page.get_by_role("button", name="Set my password")
    for name, width in WIDTHS.items():
        page.set_viewport_size({"width": width, "height": 800})
        page.goto("about:blank")
        page.goto(link)
        expect(page.locator("#code")).to_be_hidden()
        assert button.bounding_box()["height"] >= 44, name
        assert fits(page), name
        page.screenshot(path=f"{SCREENS}/reset-{name}.png", full_page=True)
    page.fill("#password", password)
    page.fill("#confirmation", password)
    button.click()
    expect(page.locator("h1")).to_have_text("Your password is changed.")
    expect(page.get_by_role("link", name="Sign in").first).to_be_visible()
    print("reset from the link")


def main():
    action, *args = sys.argv[1:]
    os.makedirs(SCREENS, exist_ok=True)
    with browsing() as page:
        {"reset": reset}[action](page, *args)


if __name__ == "__main__":
    main()
