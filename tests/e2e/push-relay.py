import json
import shlex
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from driver import credenza, subtest
    from helpers import Browser, add_member, session_token

state = "/var/lib/private/push-relay"
listen = "127.0.0.1:8098"
status_only = ["-o", "/dev/null", "-w", "%{http_code}"]


def create_device(*flags: str) -> str:
    """Asks for a device the way the web client does, for an APNs target, which the relay
    takes only once it has checked the session and then refuses as not yet built."""
    body = json.dumps({"target": {"platform": "apns", "token": "a-device-token"}})
    flags = (*status_only, "-H", "Content-Type: application/json", "--data", body, *flags)
    return credenza.succeed(shlex.join(["curl", "-s", *flags, f"http://{listen}/v1/devices"]))


with subtest("the push relay runs and its database answers its health check, on loopback alone"):
    credenza.wait_for_unit("push-relay.service")
    credenza.wait_for_open_port(8098)
    assert credenza.succeed(f"curl -sf http://{listen}/healthz") == "ok\n"
    listeners = credenza.succeed("ss -Htln 'sport = :8098'")
    assert listeners.split()[3] == listen and listeners.count("\n") == 1, listeners

with subtest("its state directory is 0700 and its database 0600, owned as other dynamic users' state"):
    # systemd keeps every dynamic user's state under one owner at rest, as concierge's.
    owner = credenza.succeed("stat -c %u /var/lib/private/concierge").strip()
    modes = credenza.succeed(f"stat -c '%a %u' {state} {state}/push-relay.sqlite3").splitlines()
    assert owner != "0" and modes == [f"700 {owner}", f"600 {owner}"], (owner, modes)

with subtest("creating a device checks the member's session token with concierge"):
    add_member("nell", "a-nest-in-the-eaves")
    nells_browser = Browser("nell")
    nells_browser.sign_in("nell", "a-nest-in-the-eaves")
    token = json.loads(session_token(nells_browser)["body"])["access_token"]
    assert create_device("-H", f"Authorization: Bearer {token}") == "501"
    assert create_device("-H", "Authorization: Bearer not-a-session-token") == "401"
    assert create_device() == "401"
