"""Sign in on members.weblounge.org, open irc.weblounge.org, and wait for
gamja to show #lounge."""

import sys

from browser import signed_in

WEBCHAT = "https://irc.weblounge.org"


def main():
    nick, password = sys.argv[1:]
    with signed_in(nick, password) as page:
        page.goto(WEBCHAT)
        page.wait_for_url(f"{WEBCHAT}/")
        page.get_by_text("#lounge").first.wait_for()
        page.get_by_text(nick).first.wait_for()
        print(page.inner_text("body"))


if __name__ == "__main__":
    main()
