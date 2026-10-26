"""Sign in, open gamja in #lounge, break its WebSocket, and have SENDER's
account say a line during the break and another while gamja is fetching
what it missed, by holding gamja's CHATHISTORY AFTER until the second
line has reached it live. Prints how many times gamja shows each line
once the first, which only the catch-up carries, is in.

gamja catches up only on channels that have had lines since the last one
it was delivered, and waits at least 10 seconds to reconnect when it
connected less than 10 seconds ago, which leaves time to say the first."""

import json
import re
import sys
import time

from browser import TIMEOUT_MS, WEBCHAT, say, signed_in

BEFORE_BREAK = "said-before-the-break"
DURING_BREAK = "said-during-the-break"
LINE = "said-while-catching-up"
CATCH_UP = re.compile(r"^(@\S+ )?CHATHISTORY AFTER ")


class CatchUpHolder:
    """Proxies gamja's WebSocket, holding back its next CHATHISTORY AFTER
    once armed until LINE has come through live."""

    def __init__(self):
        self.armed = False
        self.held = None
        self.connection = ()

    def route(self, page_side):
        server = page_side.connect_to_server()
        self.connection = (page_side, server)
        page_side.on_message(lambda message: self.from_page(server, message))
        server.on_message(lambda message: self.from_server(page_side, server, message))

    def break_connection(self):
        self.armed = True
        for side in self.connection:
            side.close(code=4000)

    def from_page(self, server, message):
        if self.armed and self.held is None and CATCH_UP.match(str(message)):
            self.held = message
        else:
            server.send(message)

    def from_server(self, page_side, server, message):
        page_side.send(message)
        if self.held is not None and f"PRIVMSG #lounge :{LINE}" in str(message):
            server.send(self.held)
            self.armed = False


def wait_until(page, condition, what):
    """Lets Playwright run the proxy's handlers until condition holds."""
    deadline = time.monotonic() + TIMEOUT_MS / 1000
    while not condition():
        if time.monotonic() > deadline:
            raise TimeoutError(f"gave up waiting for {what}")
        page.wait_for_timeout(100)


def main():
    nick, password, sender, sender_password = sys.argv[1:]
    holder = CatchUpHolder()
    with signed_in(nick, password, expected_errors=["Connection error"]) as page:
        page.route_web_socket(f"{WEBCHAT.replace('https', 'wss')}/socket", holder.route)
        page.goto(WEBCHAT)
        page.get_by_text("#lounge").first.click()
        say(sender, sender_password, BEFORE_BREAK)
        page.get_by_text(BEFORE_BREAK).wait_for()

        holder.break_connection()
        say(sender, sender_password, DURING_BREAK)
        wait_until(page, lambda: holder.held is not None, "gamja to ask what it missed")
        say(sender, sender_password, LINE)
        wait_until(page, lambda: not holder.armed, f"{LINE} to arrive live")
        page.get_by_text(DURING_BREAK).wait_for()
        shown = page.locator(".logline").all_inner_texts()
        print(json.dumps({line: sum(line in text for text in shown) for line in [DURING_BREAK, LINE]}))


if __name__ == "__main__":
    main()
