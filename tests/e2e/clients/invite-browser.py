"""Open invite links as a newcomer would: the page takes the code from the
link's fragment, says who sent it or why it won't work, and signs up. The
welcome page, which says a link to confirm the email is on its way, must
fit a phone's width and a desktop's; screenshots go to /tmp/screens."""

import os
import sys

from playwright.sync_api import expect

from browser import SCREENS, WIDTHS, browsing, fits


def main():
    link, inviter, used_link, nick, password = sys.argv[1:]
    with browsing() as page:
        page.goto(used_link)
        expect(page.locator("#code-hint")).to_contain_text("This invite doesn't work.")
        expect(page.locator("#code")).to_be_visible()

        page.goto("about:blank")
        page.goto(link)
        expect(page.locator("[data-invite-heading]")).to_have_text(f"{inviter} saved you a seat.")
        expect(page.locator("#code")).to_be_hidden()
        page.fill("#nick", nick)
        page.fill("#email", f"{nick}@example.org")
        page.fill("#password", password)
        page.fill("#confirmation", password)
        page.click("button[type=submit]")
        expect(page.locator("h1")).to_have_text(f"You're in, {nick}.")
        expect(page.locator(".notice")).to_contain_text(f"We sent a link to {nick}@example.org")
        os.makedirs(SCREENS, exist_ok=True)
        for name, width in WIDTHS.items():
            page.set_viewport_size({"width": width, "height": 800})
            assert fits(page), name
            page.screenshot(path=f"{SCREENS}/welcome-{name}.png", full_page=True)
        print("signed up from the link")


if __name__ == "__main__":
    main()
