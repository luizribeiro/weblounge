"""Sign in on members.weblounge.org, open irc.weblounge.org, and upload an
image from gamja's composer at a phone's and a desktop's width, by pasting
it and with the upload button. Prints the link each put in the composer."""

import base64
import json
import re
import sys

from playwright.sync_api import expect

from browser import WIDTHS, open_chat, signed_in

LINK = re.compile(r"^https://files\.weblounge\.org/\S+$")

PASTE = """([name, type, data]) => {
    const bytes = Uint8Array.from(atob(data), (c) => c.charCodeAt(0));
    const pasted = new DataTransfer();
    pasted.items.add(new File([bytes], name, { type }));
    const input = document.querySelector("#composer input[name=text]");
    input.focus();
    input.dispatchEvent(new ClipboardEvent("paste", { clipboardData: pasted, bubbles: true, cancelable: true }));
}"""


def main():
    nick, password, image = sys.argv[1:]
    data = base64.b64encode(open(image, "rb").read()).decode()
    links = {}
    with signed_in(nick, password) as page:
        open_chat(page)
        composer = page.locator("#composer input[name=text]")
        for name, width in WIDTHS.items():
            page.set_viewport_size({"width": width, "height": 800})
            composer.fill("")
            page.evaluate(PASTE, ["Screenshot.png", "image/png", data])
            expect(composer).to_have_value(LINK)
            links[f"{name} paste"] = composer.input_value()

            composer.fill("")
            button = page.locator("#composer-upload")
            expect(button).to_be_visible()
            with page.expect_file_chooser() as chooser:
                button.click()
            chooser.value.set_files(image)
            expect(composer).to_have_value(LINK)
            links[f"{name} button"] = composer.input_value()
    print(json.dumps(links))


if __name__ == "__main__":
    main()
