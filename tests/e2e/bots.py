import json
import re
import shlex
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from driver import client, credenza, subtest
    from helpers import (
        Account,
        add_member,
        concierge_admin,
        concierge_admin_command,
        confirm_email,
        ergo_api,
        irc,
        irc_command,
        register,
    )

luiz: Account = {"account": "luiz", "password": "plain-old-vanilla"}
alice: Account = {"account": "alice", "password": "correct-horse"}
zoe: Account = {"account": "zoe", "password": "battery-staple"}
marvin: Account = {"account": "marvin", "password": ""}


def main_pid(unit):
    return credenza.succeed(f"systemctl show -P MainPID {unit}").strip()


def in_cgroup_of(unit, command):
    procs = f"/sys/fs/cgroup/system.slice/{unit}/cgroup.procs"
    return f"sh -c 'echo $$ > {procs} && exec {command}'"


def host_of(nick):
    session = irc(f"WHOIS {nick}", **alice)
    [host] = re.findall(rf"^:\S+ 311 alice {nick} \S+ (\S+) ", session, re.M)
    return host


def wait_for_bender_in_bots_alone():
    """Once bender has rejoined after a reconnect, it's in #bots and not in #lounge."""
    names = irc_command("NAMES #bots", **alice)
    client.wait_until_succeeds(names + " | grep ' 353 alice = #bots :.*bender' >/dev/null", timeout=60)
    session = irc("NAMES #lounge", **alice)
    assert not re.search(r" 353 alice = #lounge :.*\bbender\b", session), session


def assert_kept_out_of_lounge():
    """marvin is kept out of #lounge as it connects, which Ergo does silently,
    and refused it with a 474 when it asks; it still joins #bots."""
    session = irc("JOIN #lounge", "JOIN #bots", **marvin)
    assert session.count(" 474 marvin #lounge ") == 1, session
    assert not re.search(r"^:marvin!\S+ JOIN #lounge", session, re.M), session
    assert re.search(r"^:marvin!~u@weblounge/bot/zoe JOIN #bots", session, re.M), session


def bots_browser(owner: Account, action, name):
    """Runs action on owner's bot name in a browser on the Bots tab, answering what it printed last."""
    command = shlex.join(["bots-browser", owner["account"], owner["password"], action, name])
    return client.succeed(command).strip().splitlines()[-1]


def wait_for_bender_to_answer():
    client.wait_until_succeeds(
        irc_command("PRIVMSG bender :!ping", **alice) + " | grep '^:bender!\\S* PRIVMSG alice :pong' >/dev/null",
        timeout=60,
    )


with subtest("bender waits for its owner to be a member"):
    credenza.wait_for_unit("concierge.service")
    credenza.wait_until_fails("systemctl is-active bender-account.service", timeout=60)
    credenza.fail("systemctl is-active weblounge-bot-bender.service")

with subtest("luiz becomes a member and founds #lounge, as on credenza"):
    add_member(**luiz)
    session = irc("CS REGISTER #lounge", **luiz)
    assert "Channel #lounge successfully registered" in session, session

with subtest("once its owner is a member, bender registers itself as their bot"):
    credenza.succeed("systemctl start weblounge-bot-bender.service")
    bots = concierge_admin("bot", "list", "luiz")
    assert re.match(r"bender luiz \d{4}-\d\d-\d\d$", bots), bots

with subtest("bender joins #bots marked as a bot, and answers !ping"):
    register(**alice)
    client.wait_until_succeeds(
        irc_command("JOIN #bots", "NAMES #bots", **alice) + " | grep ' 353 alice = #bots :.*bender' >/dev/null",
        timeout=60,
    )
    session = irc("PRIVMSG #bots :!ping", "WHOIS bender", **alice)
    assert re.search(r"^:bender!\S+ PRIVMSG #bots :alice: pong", session, re.M), session
    assert " 335 alice bender " in session, session

with subtest("a restart puts back bender's password, whatever it was changed to"):
    concierge_admin("bot", "reset-password", "bender")
    credenza.succeed("systemctl restart bender-account.service weblounge-bot-bender.service")
    wait_for_bender_to_answer()

with subtest("bender logs in again after its password's generator and its account restart, as a deploy may do"):
    password = credenza.succeed("cat /run/weblounge-secrets/bender-password")
    credenza.succeed("systemctl stop weblounge-secrets")
    credenza.fail("systemctl is-active weblounge-bot-bender.service")
    credenza.succeed("systemctl start weblounge-bot-bender.service")
    assert credenza.succeed("cat /run/weblounge-secrets/bender-password") == password
    wait_for_bender_to_answer()

with subtest("a deploy that changes concierge restarts it but leaves bender connected"):
    concierge, bender = main_pid("concierge"), main_pid("weblounge-bot-bender")
    credenza.succeed("/run/booted-system/specialisation/changed-concierge/bin/switch-to-configuration test")
    credenza.wait_for_unit("concierge.service")
    assert main_pid("concierge") != concierge
    assert main_pid("weblounge-bot-bender") == bender

with subtest("bender reaches credenza's loopback alone, so a bad dependency can't send its password off"):
    nginx = "curl -sk -o /dev/null --max-time 3 https://{}"
    public = credenza.succeed("ip -4 -o addr show eth1 | awk '{print $4}' | cut -d/ -f1").strip()
    credenza.succeed(in_cgroup_of("weblounge-bot-bender.service", nginx.format("127.0.0.1")))
    credenza.fail(in_cgroup_of("weblounge-bot-bender.service", nginx.format(public)))
    credenza.succeed(nginx.format(public))

with subtest("a deploy that restarts ergo and bender together starts bender once ergo is ready"):
    restarts = credenza.succeed("systemctl show -P NRestarts weblounge-bot-bender").strip()
    credenza.succeed("systemctl restart ergochat.service weblounge-bot-bender.service")
    credenza.wait_for_unit("weblounge-bot-bender.service")
    assert credenza.succeed("systemctl show -P NRestarts weblounge-bot-bender").strip() == restarts

with subtest("going back to secrets on disk, and deploying them to RAM again, keeps bender and concierge working"):
    for system in ["/run/booted-system/specialisation/secrets-on-disk", "/run/booted-system"]:
        credenza.succeed(f"{system}/bin/switch-to-configuration test")
        credenza.wait_for_unit("weblounge-bot-bender.service")
        credenza.wait_for_open_port(8089)
        assert "ergo: http://127.0.0.1:8089/ answers" in concierge_admin("status"), system
        wait_for_bender_to_answer()
        broken = credenza.succeed("systemctl list-units --no-legend --plain --state=failed,auto-restart")
        assert broken == "", (system, broken)

with subtest("bots connect from their owner's vhost, members from the network's cloak"):
    add_member(**zoe)
    marvin["password"] = concierge_admin("bot", "create", "zoe", "marvin").strip()
    assert host_of("bender") == "weblounge/bot/luiz"
    assert host_of("alice") == "weblounge"
    session = irc("WHOIS marvin", **marvin)
    assert re.search(r"^:\S+ 311 marvin marvin ~u weblounge/bot/zoe ", session, re.M), session

with subtest("once #lounge's founder bans bots' hosts, bots stay out of it but still join #bots"):
    session = irc("MODE #lounge +b *!*@weblounge/bot/*", "MODE #lounge b", **luiz)
    assert re.search(r" 367 luiz #lounge \*!\*@weblounge/bot/\* ", session), session
    credenza.succeed("systemctl restart weblounge-bot-bender.service")
    wait_for_bender_in_bots_alone()
    assert_kept_out_of_lounge()
    session = irc(**zoe)
    assert re.search(r"^:zoe!~u@weblounge JOIN #lounge", session, re.M), session

with subtest("vhosts and the ban survive an Ergo restart"):
    credenza.succeed("systemctl restart ergochat.service weblounge-bot-bender.service")
    credenza.wait_for_open_port(8089)
    wait_for_bender_in_bots_alone()
    assert host_of("bender") == "weblounge/bot/luiz"
    assert_kept_out_of_lounge()

with subtest("apply-vhosts gives bots from before vhosts theirs, connected or not"):
    for bot in ["bender", "marvin"]:
        body, _ = ergo_api("hs/set", accountName=bot, vhost="")
        assert json.loads(body) == {"success": True}, body
    assert host_of("bender") == "weblounge"
    assert concierge_admin("bot", "apply-vhosts") == "bender\nmarvin\n"
    assert host_of("bender") == "weblounge/bot/luiz"
    assert_kept_out_of_lounge()

robbie: Account = {"account": "robbie", "password": ""}

with subtest("in a browser, a member makes a bot, and it logs in with the password shown once"):
    confirm_email("zoe")
    robbie["password"] = bots_browser(zoe, "create", "robbie")
    assert re.fullmatch("[a-z2-9]{26}", robbie["password"]), robbie
    session = irc("WHOIS robbie", **robbie)
    assert " 903 " in session, session
    assert re.search(r"^:\S+ 311 robbie robbie ~u weblounge/bot/zoe ", session, re.M), session
    assert re.match(r"robbie zoe \d{4}-\d\d-\d\d$", concierge_admin("bot", "list", "zoe").splitlines()[-1])

with subtest("a new password from the Bots tab replaces the old one for good"):
    old = robbie["password"]
    robbie["password"] = bots_browser(zoe, "reset", "robbie")
    assert robbie["password"] != old
    client.fail(irc_command(account="robbie", password=old))
    session = irc(**robbie)
    assert " 903 " in session, session

with subtest("deleting a bot from the Bots tab disconnects it at once, and it can't log in again"):
    log = "/tmp/robbie.log"
    client.succeed(f"{irc_command(**robbie, linger='300')} > {log} 2>&1 &")
    client.wait_until_succeeds(f"grep -q ' 001 robbie ' {log}", timeout=30)
    assert bots_browser(zoe, "delete", "robbie") == "deleted"
    client.wait_until_succeeds(f"grep -q '^ERROR :You are no longer authorized' {log}", timeout=30)
    client.wait_until_fails("pgrep -f 'account=[r]obbie'", timeout=30)
    client.fail(irc_command(**robbie))
    body, _ = ergo_api("ns/info", accountName="robbie")
    assert json.loads(body) == {"success": False}, body
    assert "robbie" not in concierge_admin("bot", "list", "zoe")

with subtest("the passwords the Bots tab showed never reach the journal"):
    credenza.succeed("journalctl --sync")
    for secret in [old, robbie["password"]]:
        credenza.fail(f"journalctl | grep -qF {secret}")

with subtest("a deleted bot's owner makes it again at once, and it logs in from their vhost"):
    refused = credenza.fail(concierge_admin_command("bot", "create", "luiz", "robbie") + " 2>&1")
    assert "robbie is taken" in refused, refused
    deleted = robbie["password"]
    robbie["password"] = bots_browser(zoe, "create", "robbie")
    assert robbie["password"] != deleted
    session = irc("WHOIS robbie", **robbie)
    assert " 903 " in session, session
    assert re.search(r"^:\S+ 311 robbie robbie ~u weblounge/bot/zoe ", session, re.M), session
    assert re.match(r"robbie zoe \d{4}-\d\d-\d\d$", concierge_admin("bot", "list", "zoe").splitlines()[-1])
