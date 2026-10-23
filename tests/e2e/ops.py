import shlex
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from driver import credenza, subtest
    from helpers import Account, add_member, ops_lines, register, wait_for_bender, wait_for_ops_line

olive: Account = {"account": "olive", "password": "watches-the-ops-channel"}


def lines_so_far() -> int:
    return len(ops_lines(**olive))


def wait_for_line(pattern: str, since: int) -> list[str]:
    """The lines in #ops since the since'th, once one matches pattern."""
    return wait_for_ops_line(pattern, **olive, since=since)


def louro_status_as_service(*properties: str) -> str:
    flags = " ".join(f"-p {shlex.quote(p)}" for p in ["DynamicUser=yes", *properties])
    return f"systemd-run --wait --pipe --quiet {flags} -E LOURO_SOCKET=/run/louro/louro.sock louro status"


with subtest("once luiz is a member, bender sits in #ops"):
    credenza.wait_for_unit("concierge.service")
    add_member("luiz", "plain-old-vanilla")
    credenza.succeed("systemctl restart bender-account.service louro.service")
    register(**olive)
    wait_for_bender()

with subtest("root posts to #ops with ops-notify"):
    since = lines_so_far()
    credenza.succeed("ops-notify 'looking into the slow joins'")
    wait_for_line("looking into the slow joins", since)

with subtest("only the louro group reaches louro's socket"):
    credenza.succeed(louro_status_as_service("SupplementaryGroups=louro"))
    status, _ = credenza.execute(louro_status_as_service())
    assert status == 3, status
    credenza.fail("su - luiz -c 'LOURO_SOCKET=/run/louro/louro.sock louro status'")

with subtest("louro speaks in #ops alone"):
    for target in ["#bots", "#lounge", "luiz"]:
        status, output = credenza.execute(f"louro send {shlex.quote(target)} hello 2>&1")
        assert status == 5, (target, status, output)
