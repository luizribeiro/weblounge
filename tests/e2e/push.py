import re
import shlex
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from driver import client, credenza, subtest
    from helpers import Account, add_member, ops_lines, wait_for_ops_line

luiz: Account = {"account": "luiz", "password": "plain-old-vanilla"}

ssh_options = "-o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR"


def push_command(variant: str, env: str = "") -> str:
    """Pushes /etc/variants/<variant> from the client as luiz, the way just prod push does."""
    return f"{env} NIX_SSHOPTS={shlex.quote(ssh_options)} push $(readlink -f /etc/variants/{variant}) luiz@credenza"


def push(variant: str) -> tuple[int, str]:
    return client.execute(f"{push_command(variant)} 2>&1")


def running() -> str:
    return credenza.succeed("cat /etc/pushed 2>/dev/null || echo booted").strip()


def boots_into() -> str:
    return credenza.succeed("readlink -f /nix/var/nix/profiles/system")


credenza.wait_for_unit("multi-user.target")
add_member(**luiz)
credenza.succeed("systemctl restart bender-account.service louro.service")
client.succeed("ssh-keygen -q -t ed25519 -N '' -f /root/.ssh/id_ed25519")
credenza.succeed(
    "install -d -m 700 -o luiz /home/luiz/.ssh",
    f"echo {shlex.quote(client.succeed('cat /root/.ssh/id_ed25519.pub').strip())} > /home/luiz/.ssh/authorized_keys",
    "chown luiz /home/luiz/.ssh/authorized_keys",
)
booted = boots_into()

for variant, reason in [
    ("locked-out", "a fresh ssh login failed"),
    ("failing-on-host", "a check on the host failed"),
    ("failing-outside", "a check from outside failed"),
]:
    with subtest(f"a push of a system that's {variant} goes back to the one before"):
        status, output = push(variant)
        assert status != 0 and f"push: {reason}; going back to" in output, output
        assert running() == "booted" and boots_into() == booted, (running(), output)

with subtest("a push cut off mid-trial is undone by the host itself"):
    client.succeed(f"({push_command('locked-out', env='ROLLBACK_AFTER=2min')} > /tmp/cut-off.log 2>&1 &)")
    credenza.wait_until_succeeds("grep -qx locked-out /etc/pushed", timeout=600)
    client.succeed("pkill -KILL -x push")
    assert "going back" not in client.succeed("cat /tmp/cut-off.log")
    reverted = "! grep -q locked-out /etc/pushed && ! systemctl -q is-active weblounge-rollback.service"
    credenza.wait_until_succeeds(reverted, timeout=300)
    assert running() == "booted" and boots_into() == booted

with subtest("a push of a healthy system keeps it, and boots into it"):
    status, output = push("healthy")
    assert status == 0 and "push: kept" in output, output
    for site in ["weblounge.org", "members.weblounge.org", "join.weblounge.org", "irc.weblounge.org"]:
        assert f"✓ https://{site} answers" in output, (site, output)
    for check in ["Ergo's API answers its token", "IRC turns away clients that don't log in"]:
        assert f"✓ {check}" in output, (check, output)
    assert running() == "healthy" and boots_into() == client.succeed("readlink -f /etc/variants/healthy"), output

with subtest("a push of the system running already keeps it"):
    status, output = push("healthy")
    assert status == 0 and "push: kept" in output, output

with subtest("a push that can't arm its rollback gives up before switching"):
    credenza.succeed("systemd-run --unit=weblounge-rollback --on-active=1h true")
    status, output = push("failing-on-host")
    credenza.succeed("systemctl stop weblounge-rollback.timer")
    assert status != 0 and "push: couldn't arm the rollback" in output, output
    assert running() == "healthy", output

with subtest("#ops hears of every push's start, its trial, and whether it was kept or rolled back"):
    wait_for_ops_line("gave up deploying an unknown revision", **luiz)
    kinds = [
        re.sub(r" — restarted: .*", "", line)
        for line in ops_lines(**luiz)
        if re.match(r"deploying |deployed |rolled back |kept |gave up ", line)
    ]
    unknown, healthy = "an unknown revision", "22e5438"
    tried = [f"deploying {unknown} (from {unknown})", f"deployed {unknown} (on trial)", f"rolled back to {unknown}"]
    kept = [f"deploying {healthy} (from {unknown})", f"deployed {healthy} (on trial)", f"kept {healthy}"]
    gave_up = [f"deploying {unknown} (from {healthy})", f"gave up deploying {unknown}"]
    assert kinds == tried * 4 + kept + gave_up, kinds
