import re
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from driver import client, credenza, subtest
    from helpers import Account, add_member, concierge_admin, irc, irc_command, register

luiz: Account = {"account": "luiz", "password": "plain-old-vanilla"}
alice: Account = {"account": "alice", "password": "correct-horse"}


def main_pid(unit):
    return credenza.succeed(f"systemctl show -P MainPID {unit}").strip()


def in_cgroup_of(unit, command):
    procs = f"/sys/fs/cgroup/system.slice/{unit}/cgroup.procs"
    return f"sh -c 'echo $$ > {procs} && exec {command}'"


with subtest("bender waits for its owner to be a member"):
    credenza.wait_for_unit("concierge.service")
    credenza.wait_until_fails("systemctl is-active bender-account.service", timeout=60)
    credenza.fail("systemctl is-active weblounge-bot-bender.service")

with subtest("once its owner is, bender registers itself as their bot"):
    add_member(**luiz)
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
    client.wait_until_succeeds(
        irc_command("PRIVMSG bender :!ping", **alice) + " | grep '^:bender!\\S* PRIVMSG alice :pong' >/dev/null",
        timeout=60,
    )

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
