"""Sign in on members.weblounge.org, open irc.weblounge.org, pick a font
size and a font in gamja's Settings at a phone's width, then reload and
open Settings again at a desktop's. Prints, as JSON, the page's computed
font at each step and, at each width, how big and tappable the dialog's
selects are and what they show. Saves screenshots in /tmp/screens."""

import json
import os
import sys

from browser import SCREENS, WIDTHS, fits, open_chat, open_settings, signed_in

LINE = "Is this easier to read?"

FONT = """() => {
    const root = getComputedStyle(document.documentElement);
    const body = getComputedStyle(document.body);
    return { root: root.fontSize, size: body.fontSize, family: body.fontFamily };
}"""


def controls(page, settings):
    selects = {}
    for name in ["fontSize", "fontFamily"]:
        select = settings.locator(f"select[name={name}]")
        selects[name] = {
            "value": select.input_value(),
            "height": select.bounding_box()["height"],
            "font_size": select.evaluate("e => getComputedStyle(e).fontSize"),
        }
    return {"fits": fits(page), "selects": selects}


def main():
    nick, password = sys.argv[1:]
    os.makedirs(SCREENS, exist_ok=True)
    report = {}
    with signed_in(nick, password) as page:
        page.set_viewport_size({"width": WIDTHS["phone"], "height": 800})
        open_chat(page)
        page.locator("#composer input[name=text]").fill(LINE)
        page.keyboard.press("Enter")
        page.get_by_text(LINE).wait_for()
        report["default"] = page.evaluate(FONT)
        page.screenshot(path=f"{SCREENS}/webchat-chat-default-phone.png")

        settings = open_settings(page)
        report["phone default"] = controls(page, settings)
        settings.locator("select[name=fontSize]").select_option("18px")
        settings.locator("select[name=fontFamily]").select_option("sans-serif")
        report["changed"] = page.evaluate(FONT)
        report["phone"] = controls(page, settings)
        page.screenshot(path=f"{SCREENS}/webchat-settings-phone.png")
        settings.get_by_role("button", name="Close").click()

        open_chat(page)
        page.get_by_text(LINE).wait_for()
        report["reloaded"] = page.evaluate(FONT)
        page.screenshot(path=f"{SCREENS}/webchat-chat-phone.png")

        page.set_viewport_size({"width": WIDTHS["desktop"], "height": 800})
        page.screenshot(path=f"{SCREENS}/webchat-chat-desktop.png")
        settings = open_settings(page)
        report["desktop"] = controls(page, settings)
        page.screenshot(path=f"{SCREENS}/webchat-settings-desktop.png")
    print(json.dumps(report))


if __name__ == "__main__":
    main()
