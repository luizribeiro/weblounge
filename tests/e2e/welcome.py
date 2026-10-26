# The greeter, concierge, the network's host: how it runs, and what it
# says to newcomers and their inviters. It's off in every other VM test.
#
# Ergo takes 32 connections from the client in ten minutes, so the
# subtests wait for the greeter's steps in its database, and read IRC
# once a step is done.
import re
import time
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from driver import client, credenza, subtest
    from helpers import (
        Account,
        Browser,
        add_member,
        concierge_admin,
        concierge_sql,
        confirm_email,
        history_lines,
        irc,
        irc_command,
        register,
        unit_file_problems,
        wait_for_concierge_sql,
    )

greeter_units = ["concierge-greeter-account", "concierge-greeter"]
olive: Account = {"account": "olive", "password": "watches-the-lounge"}
luiz: Account = {"account": "luiz", "password": "plain-old-vanilla"}
luizs_browser = Browser("luiz")
# The greeter looks every five seconds; two looks leave it time for a step.
two_ticks = 12


def lounge() -> list[str]:
    """What the concierge said in #lounge, oldest first."""
    return history_lines("#lounge", **olive, speaker="concierge")


def told_luiz() -> list[str]:
    """What the concierge told luiz, oldest first."""
    return history_lines("concierge", **luiz, speaker="concierge")


def told(account: str) -> str:
    return f"{account} just connected for the first time and was welcomed in #lounge. Say hi!"


def nudge(account: str) -> str:
    return f"{account} hasn't connected since accepting your invite. A nudge from you might help."


def invited_by_luiz(account: str, password: str) -> None:
    """account signs up with an invite from luiz."""
    _, link = luizs_browser.create_invite()
    assert Browser(account).sign_up(link, account, password) == "200 "


def joined_minutes_ago(account: str, minutes: int) -> None:
    concierge_sql(
        f"UPDATE members SET joined_at = unixepoch() - {minutes * 60} WHERE account = '{account}'", write=True
    )


def wait_for_step(account: str, step: str) -> None:
    """Waits until the greeter recorded step, an arrivals column, for account."""
    wait_for_concierge_sql(f"SELECT {step} IS NOT NULL FROM arrivals WHERE account = '{account}'", "1")


with subtest("the greeter is in #lounge as concierge, a bot outside the members' bots' host"):
    credenza.wait_for_unit("concierge-greeter.service")
    register(**olive)
    whois = irc_command("WHOIS concierge", **olive)
    # Not grep -q, which would close the pipe before irc-client is done.
    client.wait_until_succeeds(f"{whois} | grep ' 319 olive concierge :.*#lounge'", timeout=60)
    whois = irc("WHOIS concierge", **olive)
    user = r":\S+ 311 olive concierge ~u weblounge/concierge \* :weblounge's resident bot$"
    assert re.search(user, whois, re.M), whois
    assert re.search(r":\S+ 335 olive concierge ", whois), whois

with subtest("systemd knows every setting of the greeter's units"):
    paths = [f"/etc/systemd/system/{unit}.service" for unit in greeter_units]
    assert unit_file_problems(*paths) == []

with subtest("both greeter units run sandboxed as concierge, and only the account's holds Ergo's API token"):
    for unit in greeter_units:
        credenza.succeed(f"systemd-analyze security --threshold=20 --no-pager {unit}.service")
        assert credenza.succeed(f"systemctl show -P User {unit}").strip() == "concierge"
    held = credenza.succeed("ls /run/credentials/concierge-greeter.service").split()
    assert held == ["ca", "concierge-greeter-password"], held
    credenza.succeed("systemctl cat concierge-greeter-account | grep -q '^LoadCredential=ergo-api-token:'")

with subtest("a root invite, which bootstraps the network, gets no welcome"):
    add_member(**luiz, admin=True)
    assert concierge_sql("SELECT count(*) FROM arrivals") == "0\n"

erin: Account = {"account": "erin", "password": "erin-takes-her-time"}

with subtest("nobody is welcomed while away, and the inviter of someone not seen two days in is nudged once"):
    confirm_email("luiz")
    assert luizs_browser.sign_in(**luiz).startswith("303 ")
    # Once he's connected, Ergo keeps luiz's always-on client, which takes
    # DMs while he's away.
    irc(**luiz)
    invited_by_luiz(**erin)
    joined_minutes_ago("erin", 2 * 24 * 60 + 1)
    wait_for_step("erin", "nudged_at")
    time.sleep(two_ticks)
    assert told_luiz() == [nudge("erin")], told_luiz()
    assert lounge() == [], lounge()

with subtest("a newcomer is welcomed as they first connect, and gets one hello"):
    frank: Account = {"account": "frank", "password": "frank-joins-right-away"}
    invited_by_luiz(**frank)
    session = irc(**frank, linger=str(two_ticks))
    concierge = ":concierge!~u@weblounge/concierge PRIVMSG"
    assert f"{concierge} #lounge :Welcome frank, invited by luiz!" in session, session
    wait_for_step("frank", "oriented_at")
    assert history_lines("concierge", **frank, speaker="concierge") == [
        "Hi frank! I'm concierge, weblounge's resident bot. Welcome aboard.",
        "Make sure notifications are on, so you hear when someone mentions you. On the webchat, "
        'use the "Turn on desktop notifications" link at the top or Settings, if your browser supports it.',
        "Want weblounge on your phone or desktop? These apps work well here: https://members.weblounge.org/help/apps",
        "I can't chat yet. Questions? Ask luiz, who invited you, or anyone in #lounge.",
    ]

with subtest("a restart repeats nothing, and someone who connected while it was down is welcomed at first sight"):
    welcomes = lounge()
    credenza.succeed("systemctl stop concierge-greeter")
    grace: Account = {"account": "grace", "password": "grace-keeps-it-open"}
    invited_by_luiz(**grace)
    client.succeed(f"systemd-run --unit=grace {irc_command(**grace, linger='60')}")
    credenza.wait_until_succeeds("journalctl -u ergochat -o cat | grep -q 'Client connected \\[grace\\]'")
    credenza.succeed("systemctl start concierge-greeter")
    wait_for_step("grace", "oriented_at")
    assert lounge() == [*welcomes, "Welcome grace, invited by luiz!"], lounge()
    assert told_luiz() == [nudge("erin"), told("frank"), told("grace")], told_luiz()

with subtest("a newcomer who first connects days in, after the nudge, is welcomed all the same"):
    irc(**erin, linger=str(two_ticks))
    wait_for_step("erin", "oriented_at")
    assert lounge()[-1] == "Welcome erin, invited by luiz!", lounge()
    assert told_luiz()[-1] == told("erin"), told_luiz()

with subtest("someone removed before their welcome is never welcomed"):
    invited_by_luiz("ivy", "ivy-leaves-at-once")
    concierge_admin("member", "delete", "ivy")
    assert concierge_sql("SELECT count(*) FROM arrivals WHERE account = 'ivy'") == "0\n"
    time.sleep(two_ticks)
    assert not any("ivy" in line for line in lounge()), lounge()

# It moves credenza's clock days on, so it goes last.
with subtest("on Monday at 10:00 in California, luiz is reminded once to start the week's thread"):
    california = "TZDIR=/etc/zoneinfo TZ=America/Los_Angeles"
    monday = credenza.succeed(f"{california} date -d 'next monday 10:00' +%s").strip()
    credenza.succeed(f"date -s @{monday}")
    wait_for_concierge_sql("SELECT count(*) FROM weekly_reminders", "1")
    time.sleep(two_ticks)
    lines = history_lines("concierge", **luiz, speaker="concierge")
    reminders = [line for line in lines if line.startswith("It's Monday: ")]
    assert len(reminders) == 1, lines
    assert "time for this week's thread in #lounge. A prompt you could use: " in reminders[0], reminders
