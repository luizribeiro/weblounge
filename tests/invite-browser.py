"""Open invite links as a newcomer would: the page takes the code from the
link's fragment, says who sent it or why it won't work, and signs up."""

import sys

from playwright.sync_api import expect

from browser import browsing


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
        print("signed up from the link")


if __name__ == "__main__":
    main()
