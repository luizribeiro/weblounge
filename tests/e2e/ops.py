import re
import shlex
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from driver import credenza, subtest
    from helpers import Account, add_member, ops_lines, register, wait_for_bender, wait_for_ops_line

olive: Account = {"account": "olive", "password": "watches-the-ops-channel"}
booted = r"credenza booted after \d+s \(kernel \d+\.\d+\S*\)"


def lines_so_far() -> int:
    return len(ops_lines(**olive))


def wait_since(pattern: str, since: int) -> list[str]:
    """The lines in #ops since the since'th, once one matches pattern."""
    return wait_for_ops_line(pattern, **olive, since=since)


def main_pid(unit: str) -> str:
    return credenza.succeed(f"systemctl show -P MainPID {unit}").strip()


def louro_status_as_service(*properties: str) -> str:
    flags = " ".join(f"-p {shlex.quote(p)}" for p in ["DynamicUser=yes", *properties])
    return f"systemd-run --wait --pipe --quiet {flags} -E LOURO_SOCKET=/run/louro/louro.sock louro status"


with subtest("once luiz is a member, bender sits in #ops"):
    add_member("luiz", "plain-old-vanilla")
    credenza.succeed("systemctl restart bender-account.service louro.service")
    register(**olive)
    wait_for_bender()

with subtest("root posts to #ops with ops-notify"):
    since = lines_so_far()
    credenza.succeed("ops-notify 'looking into the slow joins'")
    wait_since("looking into the slow joins", since)

with subtest("only the louro group reaches louro's socket"):
    credenza.succeed(louro_status_as_service("SupplementaryGroups=louro"))
    status, _ = credenza.execute(louro_status_as_service())
    assert status == 3, status
    credenza.fail("su - luiz -c 'LOURO_SOCKET=/run/louro/louro.sock louro status'")

with subtest("louro speaks in #ops alone"):
    for target in ["#bots", "#lounge", "luiz"]:
        status, output = credenza.execute(f"louro send {shlex.quote(target)} hello 2>&1")
        assert status == 5, (target, status, output)

with subtest("a deploy says what it deployed and restarted, and the restarts say nothing of their own"):
    since = lines_so_far()
    credenza.succeed("/run/booted-system/specialisation/deployed/bin/switch-to-configuration test")
    lines = wait_since(r"deployed 0123456 — restarted: (.*, )?concierge(, .*)?", since)
    assert not any(line.startswith("concierge") for line in lines), lines

with subtest("a crash and the restart after it say so, with the restart's count and where to look"):
    since = lines_so_far()
    credenza.succeed(f"kill -KILL {main_pid('concierge')}")
    wait_since(r"concierge crashed and restarted \(restart #1\) — journalctl -u concierge", since)

with subtest("a service that gives up restarting says so, and says when it's back"):
    since = lines_so_far()
    drop_in = "/run/systemd/system/concierge-mailer.service.d"
    failing_twice = (
        "[Unit]\\nStartLimitBurst=2\\nStartLimitIntervalSec=5min\\n"
        "[Service]\\nExecStartPre=/run/current-system/sw/bin/false\\n"
    )
    credenza.succeed(
        f"mkdir -p {drop_in}",
        f"printf '{failing_twice}' > {drop_in}/fail.conf",
        "systemctl daemon-reload",
        "systemctl kill -s KILL concierge-mailer",
    )
    credenza.wait_until_succeeds("systemctl is-failed concierge-mailer", timeout=60)
    wait_since("concierge-mailer failed — gave up restarting", since)
    credenza.succeed(
        f"rm -r {drop_in}",
        "systemctl daemon-reload",
        "systemctl reset-failed concierge-mailer",
        "systemctl start concierge-mailer",
    )
    lines = wait_since("concierge-mailer is back", since)
    assert lines == ["concierge-mailer failed — gave up restarting", "concierge-mailer is back"], lines

with subtest("lines held while louro is away go out in the order their events happened"):
    since = lines_so_far()
    credenza.succeed("systemctl stop louro.service")
    for unit in ["nginx", "concierge-mailer"]:
        credenza.succeed(f"systemctl restart {unit}")
        credenza.wait_until_succeeds(f"grep -qx started:{unit} /run/ops-events/queue/*", timeout=60)
    credenza.succeed("systemctl start louro.service")
    lines = wait_since("concierge-mailer started", since)
    assert lines == ["nginx started", "concierge-mailer started"], lines

with subtest("a clean shutdown and the boot after it say so, with the boot's length and kernel"):
    since = lines_so_far()
    credenza.shutdown()
    credenza.start()
    lines = wait_since(booted, since)
    assert lines[0] == "credenza shutting down" and re.fullmatch(booted, lines[1]), lines

with subtest("a boot after a crash says so"):
    credenza.crash()
    credenza.start(allow_reboot=True)
    # The crash may take Ergo's last lines of history with it, so this counts from the start.
    wait_since(r"credenza booted after a crash or power loss \(kernel \d+\.\d+\S*\)", since=0)

with subtest("a reboot says so, and the boot posts nothing of the services it starts"):
    since = lines_so_far()
    credenza.reboot()
    lines = wait_since(booted, since)
    assert lines[0] == "credenza rebooting" and re.fullmatch(booted, lines[1]), lines
    assert len(lines) == 2, lines
