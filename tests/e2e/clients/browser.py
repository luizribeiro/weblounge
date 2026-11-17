"""Headless Chromium on the client, on members.weblounge.org, for the tests'
Playwright scripts. Fails on any console error, which is where CSP
violations go, or uncaught page error."""

from contextlib import contextmanager

from playwright.sync_api import expect, sync_playwright

MEMBERS = "https://members.weblounge.org"
TIMEOUT_MS = 30_000
WIDTHS = {"phone": 375, "desktop": 1280}
SCREENS = "/tmp/screens"


def fits(page):
    return page.evaluate("document.documentElement.scrollWidth <= window.innerWidth")


def console_error(message):
    """The error's text and where the page logged it from."""
    location = message.location
    if not location.get("url"):
        return message.text
    return f"{message.text} (at {location['url']}:{location['lineNumber']}:{location['columnNumber']})"


def page_error(error):
    return f"uncaught {error.name}: {error.message}\n{error.stack}"


def collect_errors(page, errors):
    page.on("console", lambda message: message.type == "error" and errors.append(console_error(message)))
    page.on("pageerror", lambda error: errors.append(page_error(error)))


@contextmanager
def browsing(expected_errors=(), notifications=False, webkit=False):
    """expected_errors: substrings of console errors the caller provokes.
    notifications: run full Chromium, whose notification permission starts
    at "default" and can be granted; the headless shell always denies it.
    webkit: run WebKit, Safari's engine, which finds credenza by the names
    a test with `webkit = true;` puts in /etc/hosts."""
    errors = []
    with sync_playwright() as playwright:
        if webkit:
            browser = playwright.webkit.launch()
        else:
            browser = playwright.chromium.launch(
                channel="chromium" if notifications else None,
                args=["--host-resolver-rules=MAP *.weblounge.org credenza"],
            )
        page = browser.new_page(ignore_https_errors=True)
        page.set_default_timeout(TIMEOUT_MS)
        expect.set_options(timeout=TIMEOUT_MS)
        collect_errors(page, errors)
        yield page
        browser.close()
    errors = [error for error in errors if not any(expected in error for expected in expected_errors)]
    if errors:
        raise SystemExit("page errors:\n" + "\n".join(errors))


@contextmanager
def signed_in(nick, password, expected_errors=(), notifications=False):
    with browsing(expected_errors, notifications) as page:
        page.goto(f"{MEMBERS}/signin")
        page.fill("#nick", nick)
        page.fill("#password", password)
        page.click("button[type=submit]")
        page.wait_for_url(f"{MEMBERS}/")
        yield page
