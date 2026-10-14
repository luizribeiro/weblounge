import json
import shlex


def ergo_api_token():
    return credenza.succeed("cat /var/lib/weblounge-secrets/ergo-api-token").strip()


def ergo_api(endpoint, token=None, **fields):
    token = token or ergo_api_token()
    return credenza.succeed(shlex.join([
        "curl", "-s", "-w", "\\n%{http_code}", "-H", f"Authorization: Bearer {token}",
        "-d", json.dumps(fields), f"http://127.0.0.1:8089/v1/{endpoint}",
    ])).rsplit("\n", 1)


def register(account, password):
    body, _ = ergo_api("ns/saregister", accountName=account, passphrase=password)
    return body


def irc_command(*commands, host="credenza", **options):
    flags = [f"--{name}={value}" for name, value in options.items()]
    return shlex.join(["irc-client", host, *flags, *commands])


def irc(*commands, machine=client, **options):
    return machine.succeed(irc_command(*commands, **options))


def https(name, path="/", *flags):
    address = client.succeed("getent ahostsv4 credenza | head -1 | cut -d' ' -f1").strip()
    return client.succeed(shlex.join(
        ["curl", "-sk", "--resolve", f"{name}:443:{address}", *flags, f"https://{name}{path}"]
    ))


def assert_client_address_absent(logs):
    addresses = set(credenza.succeed("getent ahosts client | cut -d' ' -f1").split())
    patterns = shlex.join(arg for address in addresses for arg in ["-e", address])
    credenza.succeed("journalctl --sync")
    credenza.fail(f"{logs} | grep -qF {patterns}")


start_all()
client.wait_for_unit("multi-user.target")
