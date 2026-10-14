"""Sign in on members.weblounge.org in headless Chromium, open
irc.weblounge.org, and wait for gamja to show #lounge. Fails on any
console error, which is where CSP violations go."""

import sys

from playwright.sync_api import sync_playwright

MEMBERS = "https://members.weblounge.org"
WEBCHAT = "https://irc.weblounge.org"
TIMEOUT_MS = 30_000


def main():
    nick, password = sys.argv[1:]
    errors = []
    with sync_playwright() as playwright:
        browser = playwright.chromium.launch(
            args=["--host-resolver-rules=MAP *.weblounge.org credenza"],
        )
        page = browser.new_page(ignore_https_errors=True)
        page.set_default_timeout(TIMEOUT_MS)
        page.on("console", lambda message: message.type == "error"
                and errors.append(message.text))

        page.goto(f"{MEMBERS}/signin")
        page.fill("#nick", nick)
        page.fill("#password", password)
        page.click("button[type=submit]")
        page.wait_for_url(f"{MEMBERS}/")

        page.goto(WEBCHAT)
        page.wait_for_url(f"{WEBCHAT}/")
        page.get_by_text("#lounge").first.wait_for()
        page.get_by_text(nick).first.wait_for()
        print(page.inner_text("body"))
        browser.close()
    if errors:
        raise SystemExit("console errors:\n" + "\n".join(errors))


if __name__ == "__main__":
    main()
