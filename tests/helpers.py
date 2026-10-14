import json
import re
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


members = "members.weblounge.org"
outcome = ["-o", "/dev/null", "-w", "%{http_code} %{redirect_url}"]


def concierge_admin_command(*args):
    return f"su - luiz -c {shlex.quote(shlex.join(['concierge-admin', *args]))}"


def concierge_admin(*args):
    return credenza.succeed(concierge_admin_command(*args))


def invite_code(link):
    return re.split(r"[#/]", link.strip())[-1]


class Browser:
    """Someone visiting members.weblounge.org from the client, with their own cookies."""

    def __init__(self, name):
        self.cookies = ["-b", f"/tmp/cookies-{name}", "-c", f"/tmp/cookies-{name}"]

    def get(self, path, *flags):
        return https(members, path, *self.cookies, *flags)

    def post(self, path, *fields, flags=outcome):
        """Posts a form the way concierge's own pages do."""
        data = [arg for field in fields for arg in ["--data-urlencode", field]] or ["--data", ""]
        return https(members, path, *self.cookies, "-H", f"Origin: https://{members}", *flags, *data)

    def sign_in(self, account, password):
        return self.post("/signin", f"nick={account}", f"password={password}")

    def sign_up(self, link, account, password, flags=outcome):
        """Fills in the invite page with the code from `link`, as its script does."""
        fields = [f"code={invite_code(link)}", f"nick={account}", f"email={account}@example.org",
                  f"password={password}", f"confirmation={password}"]
        return self.post("/invite", *fields, flags=flags)

    def create_invite(self):
        """Answers with the page of a new invite and its link, which nothing shows again."""
        page = self.post("/invites", flags=["-f"])
        [link] = re.findall(r'<code class="invite-link">([^<]+)</code>', page)
        return page, link


def add_member(account, password, admin=False):
    """Signs `account` up through a root invite, the way every member joins."""
    link = concierge_admin("invite", "create", "--root", *(["--admin"] if admin else []))
    signed_up = Browser(f"signup-{account}").sign_up(link, account, password)
    assert signed_up == "200 ", signed_up


def client_addresses():
    return set(credenza.succeed("getent ahosts client | cut -d' ' -f1").split())


def patterns(texts):
    return shlex.join(arg for text in texts for arg in ["-e", text])


def assert_client_address_absent(logs):
    credenza.succeed("journalctl --sync")
    credenza.fail(f"{logs} | grep -qF {patterns(client_addresses())}")


def files_containing(*texts):
    """Files under /var/lib and /var/log, the journal's aside, that hold any of texts."""
    credenza.succeed("sync")
    return credenza.succeed(
        f"grep -rlaF --exclude-dir=journal {patterns(texts)} /var/lib /var/log || true"
    ).split()


start_all()
client.wait_for_unit("multi-user.target")
