"""Check that the Account tab fits at a phone's width and a desktop's, that
its Save button turns on only while its form differs from what was saved,
and that the Email card asks for the password only while the address
differs. Saves screenshots in SCREENS."""

import os
import sys

from playwright.sync_api import expect

from browser import MEMBERS, SCREENS, WIDTHS, fits, signed_in


def main():
    nick, password = sys.argv[1:]
    with signed_in(nick, password) as page:
        page.goto(f"{MEMBERS}/account")
        os.makedirs(SCREENS, exist_ok=True)
        for name, width in WIDTHS.items():
            page.set_viewport_size({"width": width, "height": 800})
            assert fits(page), f"the Account tab overflows at {width}px"
            page.screenshot(path=f"{SCREENS}/account-{name}.png", full_page=True)

        email_save = page.locator('form[action="/settings"] > button[type=submit]')
        email = page.locator("#email")
        saved_email = email.input_value()

        expect(email_save).to_be_disabled()

        email_password = page.locator("#email-password")
        expect(email_password).to_be_hidden()
        email.fill("someone-else@example.org")
        expect(email_save).to_be_enabled()
        expect(email_password).to_be_visible()
        assert email_password.evaluate("input => input.required")
        email.fill(saved_email)
        expect(email_save).to_be_disabled()
        expect(email_password).to_be_hidden()
        assert not email_password.evaluate("input => input.required")

        page.locator('input[name="news"]').click()
        email_save.click()
        page.wait_for_url(f"{MEMBERS}/account?saved#email-and-news")
        expect(page.locator(".notice[role=status]")).to_have_text("Saved.")
        expect(email_save).to_be_disabled()
        print("the Account tab fits, and its save button and password field follow changes")


if __name__ == "__main__":
    main()
