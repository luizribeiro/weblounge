"""Make, reset or delete a bot on the Bots tab, as its owner would, at a
phone's width or a desktop's: `create NAME`, `reset NAME` or `delete
NAME`. Creating and resetting print the password the dialog shows, once.
`network NAME` checks, at both widths, that the service bot NAME is
labelled a network bot with nothing to change it, saves screenshots in
/tmp/screens and prints the tab's count.
Checks that the name field calls the name free as it's typed, that the
dialogs ask first, that the controls fit and are big
enough to tap, and that the password is gone once the dialog is."""

import os
import sys

from playwright.sync_api import Locator, Page, expect

from browser import MEMBERS, SCREENS, WIDTHS, fits, signed_in

ONCE = "Copy it now. It won't be shown again; if you lose it, make a new one."


def assert_tappable(control: Locator, width: str) -> None:
    box = control.bounding_box()
    assert box and box["height"] >= 44, (width, box)


def shown_password(page: Page, width: str) -> str:
    dialog = page.locator("dialog#bot-password")
    expect(dialog).to_be_visible()
    assert dialog.evaluate("dialog => dialog.matches(':modal')"), width
    expect(dialog.get_by_text(ONCE)).to_be_visible()
    password = dialog.locator("dd code").nth(1).inner_text()
    done = dialog.get_by_role("link", name="Done")
    assert_tappable(done, width)
    assert fits(page), width
    done.click()
    page.wait_for_url(f"{MEMBERS}/bots")
    assert password not in page.content(), width
    return password


def confirm(page: Page, dialog_id: str, button: str, width: str) -> None:
    page.locator(f'a[data-dialog="{dialog_id}"]').click()
    dialog = page.locator(f"dialog#{dialog_id}")
    expect(dialog).to_be_visible()
    expect(dialog.get_by_role("button", name="Keep it")).to_be_focused()
    confirmed = dialog.get_by_role("button", name=button)
    assert_tappable(confirmed, width)
    assert fits(page), width
    with page.expect_navigation():
        confirmed.click()


def assert_network_bot(page: Page, name: str) -> str:
    """Checks the tab at each width shows the service bot `name` read-only; answers its count."""
    os.makedirs(SCREENS, exist_ok=True)
    for width in ["phone", "desktop"]:
        page.set_viewport_size({"width": WIDTHS[width], "height": 800})
        bot = page.locator("li.bot", has=page.get_by_text(name, exact=True))
        expect(bot.locator(".bot-label")).to_have_text("network bot")
        expect(bot.get_by_role("link")).to_have_count(0)
        expect(page.locator(f'[data-dialog$="-{name}"]')).to_have_count(0)
        expect(page.get_by_role("button", name="Make a bot")).to_have_count(0)
        assert fits(page), width
        page.screenshot(path=f"{SCREENS}/bots-network-{width}.png", full_page=True)
    return " ".join(page.locator(".invite-count").inner_text().split())


def main():
    nick, password, action, name = sys.argv[1:]
    width = "phone" if action != "reset" else "desktop"
    with signed_in(nick, password) as page:
        page.set_viewport_size({"width": WIDTHS[width], "height": 800})
        page.goto(f"{MEMBERS}/bots")
        assert fits(page), width
        if action == "create":
            page.fill("#bot-name", name)
            expect(page.locator("#bot-name-hint")).to_have_text(f"{name} is free.")
            make = page.get_by_role("button", name="Make a bot")
            assert_tappable(make, width)
            make.click()
            print(shown_password(page, width))
        elif action == "reset":
            confirm(page, f"reset-{name}", "Make a new password", width)
            print(shown_password(page, width))
        elif action == "delete":
            confirm(page, f"delete-{name}", f"Delete {name}", width)
            assert page.url == f"{MEMBERS}/bots", page.url
            expect(page.get_by_text(name, exact=True)).to_have_count(0)
            print("deleted")
        elif action == "network":
            print(assert_network_bot(page, name))


if __name__ == "__main__":
    main()
