"""Check, at a phone's width and a desktop's, that a member with an open
invite revokes it from the Invites tab after confirming, and that the
controls fit the page and are big enough to tap. Saves screenshots in
/tmp/screens."""

import os
import sys

from playwright.sync_api import expect

from browser import MEMBERS, signed_in

WIDTHS = {"phone": 375, "desktop": 1280}
SCREENS = "/tmp/screens"


def fits(page):
    return page.evaluate("document.documentElement.scrollWidth <= window.innerWidth")


def main():
    nick, password = sys.argv[1:]
    os.makedirs(SCREENS, exist_ok=True)
    with signed_in(nick, password) as page:
        for name, width in WIDTHS.items():
            page.set_viewport_size({"width": width, "height": 800})
            page.goto(f"{MEMBERS}/invites")
            page.get_by_role("button", name="Invite someone").click()
            page.goto(f"{MEMBERS}/invites")
            revoke = page.get_by_role("link", name="Revoke all")
            expect(revoke).to_be_visible()
            assert revoke.bounding_box()["height"] >= 44, name
            assert fits(page), name
            page.screenshot(path=f"{SCREENS}/invites-{name}.png", full_page=True)

            revoke.click()
            confirm = page.get_by_role("button", name="Revoke all")
            expect(confirm).to_be_visible()
            assert confirm.bounding_box()["height"] >= 44, name
            assert fits(page), name
            page.screenshot(path=f"{SCREENS}/revoke-{name}.png", full_page=True)

            confirm.click()
            page.wait_for_url(f"{MEMBERS}/invites")
            expect(page.get_by_text("open invite")).to_have_count(0)
        print("revoking fits and works at both widths")


if __name__ == "__main__":
    main()
