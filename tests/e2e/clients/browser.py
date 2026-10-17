"""Headless Chromium on the client, on members.weblounge.org, for the tests'
Playwright scripts. Fails on any console error, which is where CSP
violations go."""

from contextlib import contextmanager

from playwright.sync_api import expect, sync_playwright

MEMBERS = "https://members.weblounge.org"
TIMEOUT_MS = 30_000
WIDTHS = {"phone": 375, "desktop": 1280}
SCREENS = "/tmp/screens"


def fits(page):
    return page.evaluate("document.documentElement.scrollWidth <= window.innerWidth")


@contextmanager
def browsing(expected_errors=()):
    """expected_errors: substrings of console errors the caller provokes."""
    errors = []
    with sync_playwright() as playwright:
        browser = playwright.chromium.launch(args=["--host-resolver-rules=MAP *.weblounge.org credenza"])
        page = browser.new_page(ignore_https_errors=True)
        page.set_default_timeout(TIMEOUT_MS)
        expect.set_options(timeout=TIMEOUT_MS)
        page.on("console", lambda message: message.type == "error" and errors.append(message.text))
        yield page
        browser.close()
    errors = [error for error in errors if not any(expected in error for expected in expected_errors)]
    if errors:
        raise SystemExit("console errors:\n" + "\n".join(errors))


@contextmanager
def signed_in(nick, password, expected_errors=()):
    with browsing(expected_errors) as page:
        page.goto(f"{MEMBERS}/signin")
        page.fill("#nick", nick)
        page.fill("#password", password)
        page.click("button[type=submit]")
        page.wait_for_url(f"{MEMBERS}/")
        yield page
