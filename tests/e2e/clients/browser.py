"""Headless Chromium on the client, on members.weblounge.org, for the tests'
Playwright scripts. Fails on any console error, which is where CSP
violations go."""

import subprocess
from contextlib import contextmanager

from playwright.sync_api import expect, sync_playwright

MEMBERS = "https://members.weblounge.org"
WEBCHAT = "https://irc.weblounge.org"
TIMEOUT_MS = 30_000
WIDTHS = {"phone": 375, "desktop": 1280}
SCREENS = "/tmp/screens"


def fits(page):
    return page.evaluate("document.documentElement.scrollWidth <= window.innerWidth")


def open_chat(page):
    """Opens gamja and waits for it to land in #lounge."""
    page.goto(WEBCHAT)
    page.wait_for_url(f"{WEBCHAT}/")
    page.get_by_text("#lounge").first.wait_for()


def say(account, password, line):
    """Has ACCOUNT say LINE in #lounge, from a connection of its own."""
    command = ["irc-client", "credenza", f"--account={account}", f"--password={password}", f"PRIVMSG #lounge :{line}"]
    subprocess.run(command, check=True, stdout=subprocess.DEVNULL)


def open_settings(page):
    """Opens gamja's Settings, from the server buffer, and returns its form."""
    expander = page.locator("#buffer-list > .expander")
    if expander.is_visible():
        expander.click()
    page.locator("#buffer-list li.type-server a").click()
    page.get_by_role("button", name="Settings").click()
    return page.locator(".dialog form")


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
        page.on("console", lambda message: message.type == "error" and errors.append(message.text))
        yield page
        browser.close()
    errors = [error for error in errors if not any(expected in error for expected in expected_errors)]
    if errors:
        raise SystemExit("console errors:\n" + "\n".join(errors))


@contextmanager
def signed_in(nick, password, expected_errors=(), notifications=False):
    with browsing(expected_errors, notifications) as page:
        page.goto(f"{MEMBERS}/signin")
        page.fill("#nick", nick)
        page.fill("#password", password)
        page.click("button[type=submit]")
        page.wait_for_url(f"{MEMBERS}/")
        yield page
