"""In WebKit, open beta.weblounge.org signed out of members.weblounge.org
and sign in where the beta client sends us, then open it again, signed
in, and once more in a browser that keeps only members' cookies. Prints
the pages each visit went through, where it ended, and whether the
client showed #lounge.

WebKit, because Chromium's ignore_https_errors doesn't reach the
client's SharedWorker, whose WebSocket then fails on the test's
certificate."""

import json
import sys

from playwright.sync_api import TimeoutError as PlaywrightTimeout

from browser import MEMBERS, TIMEOUT_MS, browsing

BETA = "https://beta.weblounge.org"


def visit(page, sign_in=None):
    visited = []

    def record(frame):
        if frame == page.main_frame:
            visited.append(frame.url)

    page.on("framenavigated", record)
    page.goto(f"{BETA}/", wait_until="commit")
    if sign_in:
        page.wait_for_url(f"{MEMBERS}/signin?**")
        sign_in(page)
    try:
        page.get_by_text("#lounge").first.wait_for()
        lounge = True
    except PlaywrightTimeout:
        lounge = False
    page.remove_listener("framenavigated", record)
    return {"visited": visited, "url": page.url, "lounge": lounge}


def with_members_cookies_only(page):
    context = page.context.browser.new_context(ignore_https_errors=True)
    context.add_cookies(page.context.cookies(MEMBERS))
    fresh = context.new_page()
    fresh.set_default_timeout(TIMEOUT_MS)
    return fresh


def main():
    nick, password = sys.argv[1:]

    def sign_in(page):
        page.fill("#nick", nick)
        page.fill("#password", password)
        page.click("button[type=submit]")

    # A signed-out launch learns it must sign in from the session-token
    # endpoint's 401, which WebKit logs as a failed load.
    with browsing(expected_errors=["status of 401 (Unauthorized)"], webkit=True) as page:
        report = {"signed out": visit(page, sign_in)}
        page.goto("about:blank")
        report["signed in"] = visit(page)
        report["members cookies only"] = visit(with_members_cookies_only(page))
        print(json.dumps(report), flush=True)


if __name__ == "__main__":
    main()
