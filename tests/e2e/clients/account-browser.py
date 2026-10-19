"""Check that the Account tab's Save buttons turn on only while their form
differs from what was saved, and that the Email card asks for the password
only while the address differs."""

import sys

from playwright.sync_api import expect

from browser import MEMBERS, signed_in


def main():
    nick, password = sys.argv[1:]
    with signed_in(nick, password) as page:
        page.goto(f"{MEMBERS}/account")
        email_save = page.locator('form[action="/settings"] > button[type=submit]')
        irc_save = page.locator('form[action="/irc-settings"] > button[type=submit]')
        email = page.locator("#email")
        always_on = page.locator('input[name="always_on"]')
        saved_email = email.input_value()

        expect(email_save).to_be_disabled()
        expect(irc_save).to_be_disabled()

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

        always_on.click()
        expect(irc_save).to_be_enabled()
        expect(email_save).to_be_disabled()
        always_on.click()
        expect(irc_save).to_be_disabled()

        page.locator('input[name="news"]').click()
        email_save.click()
        page.wait_for_url(f"{MEMBERS}/account?saved#email-and-news")
        expect(page.locator(".notice[role=status]")).to_have_text("Saved.")
        expect(email_save).to_be_disabled()
        print("save buttons and the email's password field follow changes")


if __name__ == "__main__":
    main()
