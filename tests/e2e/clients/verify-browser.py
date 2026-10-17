"""Open a verification link at a phone's width and a desktop's: `look`
checks the page offers a Confirm button that fits and is big enough to
tap, with the code field showing only without JavaScript or for a link
cut short; `confirm` presses it. `resend` signs in and asks the Account
tab for a new link. `unconfirmed` signs in before confirming and checks
the reminder on Home and the Invites tab's reason fit and lead to the
Account tab. Saves screenshots in /tmp/screens."""

import os
import sys

from playwright.sync_api import expect

from browser import MEMBERS, SCREENS, WIDTHS, browsing, fits, signed_in


def confirm_button(page):
    return page.get_by_role("button", name="Confirm my email")


def visit(page, url):
    """Loads `url` afresh: from a URL it differs from only in the fragment,
    a plain goto would keep the page and not run its script again."""
    page.goto("about:blank")
    page.goto(url)


def look(page, link):
    for name, width in WIDTHS.items():
        page.set_viewport_size({"width": width, "height": 800})
        visit(page, link)
        expect(page.locator("#code")).to_be_hidden()
        assert confirm_button(page).bounding_box()["height"] >= 44, name
        assert fits(page), name
        page.screenshot(path=f"{SCREENS}/verify-{name}.png", full_page=True)

        visit(page, link[:-1])
        expect(page.locator("#code-hint")).to_contain_text("This link looks cut short.")
        expect(page.locator("#code")).to_be_visible()

    without_js = page.context.browser.new_page(ignore_https_errors=True, java_script_enabled=False)
    for name, width in WIDTHS.items():
        without_js.set_viewport_size({"width": width, "height": 800})
        without_js.goto(link)
        code = without_js.locator("#code")
        expect(code).to_be_visible()
        assert code.evaluate("input => parseFloat(getComputedStyle(input).fontSize)") >= 16, name
        assert code.bounding_box()["height"] >= 44, name
        assert fits(without_js), name
        without_js.screenshot(path=f"{SCREENS}/verify-no-js-{name}.png", full_page=True)
    print("the page fits at both widths and verifies nothing by loading")


def confirm(page, link):
    page.set_viewport_size({"width": WIDTHS["phone"], "height": 800})
    page.goto(link)
    confirm_button(page).click()
    expect(page.locator("h1")).to_have_text("Your email is confirmed.")
    for name, width in WIDTHS.items():
        page.set_viewport_size({"width": width, "height": 800})
        assert page.get_by_role("link", name="Go to your account").bounding_box()["height"] >= 44, name
        assert fits(page), name
        page.screenshot(path=f"{SCREENS}/verified-{name}.png", full_page=True)
    print("confirmed from the link")


def resend(page):
    """The verification pages' "Send a new link" leads to the button."""
    button = page.get_by_role("button", name="Send a new link")
    for name, width in WIDTHS.items():
        page.set_viewport_size({"width": width, "height": 800})
        visit(page, f"{MEMBERS}/account#email-and-news")
        expect(page.locator(".email-status")).to_contain_text("Please confirm by")
        expect(button).to_be_in_viewport()
        assert button.bounding_box()["height"] >= 44, name
        assert fits(page), name
        page.screenshot(path=f"{SCREENS}/account-{name}.png", full_page=True)
    button.click()
    page.wait_for_url(f"{MEMBERS}/account?sent#email-and-news")
    expect(page.locator(".notice[role=status]")).to_contain_text("A link is on its way.")
    page.screenshot(path=f"{SCREENS}/account-sent-desktop.png", full_page=True)
    print("asked for a new link")


def unconfirmed(page):
    reminder = page.locator(".email-reminder")
    for name, width in WIDTHS.items():
        page.set_viewport_size({"width": width, "height": 800})
        page.goto(f"{MEMBERS}/")
        expect(reminder).to_contain_text("Please confirm your email by")
        assert reminder.get_by_role("link", name="Send a new link").bounding_box()["height"] >= 44, name
        assert fits(page), name
        page.screenshot(path=f"{SCREENS}/banner-{name}.png", full_page=True)

        page.goto(f"{MEMBERS}/invites")
        expect(page.get_by_text("You can't invite others until you confirm your email.")).to_be_visible()
        expect(reminder).to_have_count(0)
        confirm_link = page.get_by_role("link", name="Confirm your email")
        assert confirm_link.bounding_box()["height"] >= 44, name
        assert fits(page), name
        page.screenshot(path=f"{SCREENS}/invites-unverified-{name}.png", full_page=True)
    confirm_link.click()
    page.wait_for_url(f"{MEMBERS}/account#email-and-news")
    expect(reminder).to_have_count(0)
    print("the reminder and the Invites tab lead to the Account tab")


def main():
    action, *args = sys.argv[1:]
    os.makedirs(SCREENS, exist_ok=True)
    if action in ("resend", "unconfirmed"):
        with signed_in(*args) as page:
            {"resend": resend, "unconfirmed": unconfirmed}[action](page)
        return
    with browsing() as page:
        {"look": look, "confirm": confirm}[action](page, *args)


if __name__ == "__main__":
    main()
