# The greeter, concierge, the network's host: how it runs, and what it
# says to newcomers and their inviters. It's off in every other VM test.
import re
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from driver import client, credenza, subtest
    from helpers import Account, irc, irc_command, register, unit_file_problems

greeter_units = ["concierge-greeter-account", "concierge-greeter"]
olive: Account = {"account": "olive", "password": "watches-the-lounge"}

with subtest("the greeter is in #lounge as concierge, a bot outside the members' bots' host"):
    credenza.wait_for_unit("concierge-greeter.service")
    register(**olive)
    whois = irc_command("WHOIS concierge", **olive)
    # Not grep -q, which would close the pipe before irc-client is done.
    client.wait_until_succeeds(f"{whois} | grep ' 319 olive concierge :.*#lounge'", timeout=60)
    whois = irc("WHOIS concierge", **olive)
    assert re.search(r":\S+ 311 olive concierge ~u weblounge/concierge \* ", whois), whois
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
