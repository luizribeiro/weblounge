import json
import shlex
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from test_driver.machine import QemuMachine

    from driver import client, credenza, subtest
    from helpers import Browser, add_member, https, session_token

push = "push.weblounge.org"
state = "/var/lib/private/push-relay"
listen = "127.0.0.1:8098"
status_only = ["-o", "/dev/null", "-w", "%{http_code}"]


def statuses(machine: "QemuMachine", address: str, paths: list[str], *flags: str) -> list[str]:
    """What push.weblounge.org answers each path, asked in turn over one connection to nginx at address."""
    urls = [f"https://{push}{path}" for path in paths]
    resolve = ["--resolve", f"{push}:443:{address}"]
    outputs = ["-w", "%{http_code}\\n", *["-o", "/dev/null"] * len(urls)]
    return machine.succeed(shlex.join(["curl", "-sk", *resolve, *outputs, *flags, *urls])).split()


def create_device(*flags: str) -> str:
    """Asks for a device the way the web client does, for an APNs target, which the relay
    takes only once it has checked the session and then refuses as not yet built."""
    body = json.dumps({"target": {"platform": "apns", "token": "a-device-token"}})
    return https(push, "/v1/devices", *status_only, "-H", "Content-Type: application/json", "--data", body, *flags)


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

with subtest("push.weblounge.org answers through nginx with the relay's own answers"):
    assert https(push, "/healthz") == "ok\n"
    assert https(push, "/no-such-path", *status_only) == "404"
    assert https(push, "/v1/r/no-such-route", *status_only, "--data-binary", "x") == "404"

with subtest("nginx refuses a home's push over 4 KB before the relay sees it"):
    body = "x" * 5000
    assert https(push, "/v1/r/x", *status_only, "--data-binary", body) == "413"

with subtest("creating a device checks the member's session token with concierge"):
    add_member("nell", "a-nest-in-the-eaves")
    nells_browser = Browser("nell")
    nells_browser.sign_in("nell", "a-nest-in-the-eaves")
    token = json.loads(session_token(nells_browser)["body"])["access_token"]
    assert create_device("-H", f"Authorization: Bearer {token}") == "501"
    assert create_device("-H", "Authorization: Bearer not-a-session-token") == "401"
    assert create_device() == "401"

credenza_from_client = client.succeed("getent ahostsv4 credenza | head -1 | cut -d' ' -f1").strip()
push_body = ["--data-binary", "a-push"]

with subtest("nginx holds the device API to its rate per address"):
    answers = statuses(client, credenza_from_client, ["/healthz"] * 40)
    assert answers[0] == "200" and "429" in answers, answers

with subtest("nginx holds homes to a rate per address, but for credenza's own, where Ergo pushes from"):
    routes = [f"/v1/r/route-{n}" for n in range(300)]
    assert "429" in statuses(client, credenza_from_client, routes, *push_body)
    assert set(statuses(credenza, "127.0.0.1", routes, *push_body)) == {"404"}

with subtest("nginx holds each route to a rate, credenza's own pushes too"):
    answers = statuses(credenza, "127.0.0.1", ["/v1/r/one-route"] * 150, *push_body)
    assert answers[0] == "404" and "429" in answers, answers
