"""Sign in on members.weblounge.org, open irc.weblounge.org, and paste an
image into gamja's composer at a phone's and a desktop's width. Prints
the link each paste put in the composer."""

import base64
import json
import re
import sys

from playwright.sync_api import expect

from browser import WIDTHS, signed_in

WEBCHAT = "https://irc.weblounge.org"
LINK = "https://files.weblounge.org/"

PASTE = """([name, type, data]) => {
    const bytes = Uint8Array.from(atob(data), (c) => c.charCodeAt(0));
    const pasted = new DataTransfer();
    pasted.items.add(new File([bytes], name, { type }));
    const input = document.querySelector("#composer input");
    input.focus();
    input.dispatchEvent(new ClipboardEvent("paste", { clipboardData: pasted, bubbles: true, cancelable: true }));
}"""


def main():
    nick, password, image = sys.argv[1:]
    data = base64.b64encode(open(image, "rb").read()).decode()
    links = {}
    with signed_in(nick, password) as page:
        page.goto(WEBCHAT)
        page.wait_for_url(f"{WEBCHAT}/")
        page.get_by_text("#lounge").first.wait_for()
        composer = page.locator("#composer input")
        for name, width in WIDTHS.items():
            page.set_viewport_size({"width": width, "height": 800})
            composer.fill("")
            page.evaluate(PASTE, ["Screenshot.png", "image/png", data])
            expect(composer).to_have_value(re.compile(re.escape(LINK)))
            links[name] = composer.input_value()
    print(json.dumps(links))


if __name__ == "__main__":
    main()
