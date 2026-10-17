"""Check that the Account tab's Save buttons turn on only while their form
differs from what was saved."""

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

        email.fill("someone-else@example.org")
        expect(email_save).to_be_enabled()
        email.fill(saved_email)
        expect(email_save).to_be_disabled()

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
        print("save buttons follow changes")


if __name__ == "__main__":
    main()
