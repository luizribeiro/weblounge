"""Sign in on members.weblounge.org, open irc.weblounge.org, and wait for
gamja to show #lounge."""

import sys

from browser import open_chat, signed_in


def main():
    nick, password = sys.argv[1:]
    with signed_in(nick, password) as page:
        open_chat(page)
        page.get_by_text(nick).first.wait_for()
        print(page.inner_text("body"))


if __name__ == "__main__":
    main()
