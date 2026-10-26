"""Sign in and open gamja twice in full Chromium, which, unlike the headless
shell, has notification permissions. The first time the page may ask for
them: look at gamja's nagger in #lounge, at a phone's and a desktop's
width, and in the server buffer, dismiss it and reopen gamja, then turn on
Desktop notifications in Settings, which the headless browser refuses. The
second time they're granted: have SENDER mention the member with Desktop
notifications on, off after reopening gamja, and on again, recording the
notifications gamja shows. Prints what it saw, as JSON, and saves
screenshots in /tmp/screens."""

import json
import os
import sys

from browser import SCREENS, WEBCHAT, WIDTHS, fits, open_chat, open_settings, say, signed_in

RECORD_NOTIFICATIONS = """() => {
    const Real = window.Notification;
    window.shownNotifications = [];
    window.Notification = class extends Real {
        constructor(title, options) {
            super(title, options);
            window.shownNotifications.push(title);
        }
    };
}"""


def at_width(page, width):
    page.set_viewport_size({"width": WIDTHS[width], "height": 800})


def open_lounge(page):
    expander = page.locator("#buffer-list > .expander")
    if expander.is_visible():
        expander.click()
    page.locator("#buffer-list li", has_text="#lounge").locator("a").click()


def nagger(page):
    nag = page.locator(".notification-nag")
    if not nag.is_visible():
        return None
    link = nag.get_by_role("link")
    dismiss = nag.get_by_role("button")
    return {
        "text": " ".join(nag.inner_text().split()),
        "link": link.inner_text(),
        "link_height": link.bounding_box()["height"],
        "dismiss": dismiss.inner_text(),
        "dismiss_height": dismiss.bounding_box()["height"],
        "fits": fits(page),
    }


TOGGLE = "input[name=desktopNotifications]"


def toggle(page):
    return page.locator(f".dialog form {TOGGLE}")


def read_settings(page):
    label = page.locator(".dialog form label", has=page.locator(TOGGLE))
    blocked = page.locator(".dialog form .notifications-blocked")
    return {
        "label": label.inner_text().strip(),
        "label_height": label.bounding_box()["height"],
        "checked": toggle(page).is_checked(),
        "blocked": " ".join(blocked.inner_text().split()) if blocked.count() else None,
        "fits": fits(page),
    }


def close_settings(page):
    page.locator(".dialog form").get_by_role("button", name="Close").click()


def asking(nick, password):
    report = {}
    with signed_in(nick, password, notifications=True) as page:
        report["permission"] = page.evaluate("Notification.permission")
        for width in WIDTHS:
            at_width(page, width)
            open_chat(page)
            open_lounge(page)
            report[f"{width} lounge"] = nagger(page)
            page.screenshot(path=f"{SCREENS}/webchat-nagger-{width}.png")
        open_settings(page)
        close_settings(page)
        report["server buffer"] = nagger(page)

        open_lounge(page)
        page.locator(".notification-nag").get_by_role("button", name="Dismiss").click()
        report["dismissed"] = nagger(page)
        open_chat(page)
        open_lounge(page)
        report["reopened"] = nagger(page)

        open_settings(page)
        report["settings"] = read_settings(page)
        toggle(page).click()
        page.locator(".dialog form .notifications-blocked").wait_for()
        report["permission after asking"] = page.evaluate("Notification.permission")
        for width in WIDTHS:
            at_width(page, width)
            report[f"{width} denied"] = read_settings(page)
            page.screenshot(path=f"{SCREENS}/webchat-notifications-denied-{width}.png")
    return report


def granted(nick, password, sender, sender_password):
    report = {}
    with signed_in(nick, password, notifications=True) as page:
        page.context.grant_permissions(["notifications"], origin=WEBCHAT)
        page.add_init_script(f"({RECORD_NOTIFICATIONS})()")

        def mention(line):
            open_lounge(page)
            say(sender, sender_password, f"{nick}: {line}")
            page.get_by_text(line).wait_for()
            return page.evaluate("window.shownNotifications")

        open_chat(page)
        report["nagger"] = nagger(page)
        open_settings(page)
        report["on"] = read_settings(page)
        page.screenshot(path=f"{SCREENS}/webchat-notifications-on-desktop.png")
        close_settings(page)
        report["shown when on"] = mention("are you there?")

        open_settings(page)
        toggle(page).click()
        close_settings(page)
        open_chat(page)
        open_settings(page)
        report["off after reopening"] = read_settings(page)
        close_settings(page)
        report["shown when off"] = mention("still there?")

        open_settings(page)
        toggle(page).click()
        report["on again"] = read_settings(page)
        close_settings(page)
        report["shown when on again"] = mention("back again?")
    return report


def main():
    nick, password, sender, sender_password = sys.argv[1:]
    os.makedirs(SCREENS, exist_ok=True)
    report = {"asking": asking(nick, password), "granted": granted(nick, password, sender, sender_password)}
    print(json.dumps(report))


if __name__ == "__main__":
    main()
