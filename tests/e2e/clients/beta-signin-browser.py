"""In WebKit, open beta.weblounge.org signed out of members.weblounge.org
and sign in where the beta client sends us, then open it again, signed
in. Prints the pages each visit went through, where it ended, and
whether the client showed #lounge.

WebKit, because Chromium's ignore_https_errors doesn't reach the
client's SharedWorker, whose WebSocket then fails on the test's
certificate."""

import json
import sys

from playwright.sync_api import TimeoutError as PlaywrightTimeout

from browser import MEMBERS, browsing

BETA = "https://beta.weblounge.org"


def visit(page, sign_in=None):
    visited = []

    def record(frame):
        if frame == page.main_frame:
            visited.append(frame.url)

    page.on("framenavigated", record)
    page.goto(f"{BETA}/")
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


def main():
    nick, password = sys.argv[1:]

    def sign_in(page):
        page.fill("#nick", nick)
        page.fill("#password", password)
        page.click("button[type=submit]")

    with browsing(webkit=True) as page:
        report = {"signed out": visit(page, sign_in)}
        page.goto("about:blank")
        report["signed in"] = visit(page)
        print(json.dumps(report), flush=True)


if __name__ == "__main__":
    main()
