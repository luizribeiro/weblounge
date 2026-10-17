import json
import re
import shlex
import time
from collections.abc import Iterable, Sequence
from typing import TYPE_CHECKING, Any, TypedDict
from urllib.parse import quote

if TYPE_CHECKING:
    from test_driver.machine import QemuMachine

    from driver import client, credenza, mail, start_all, tools


class Account(TypedDict):
    account: str
    password: str


def ergo_api_token() -> str:
    return credenza.succeed("cat /run/weblounge-secrets/ergo-api-token").strip()


def ergo_api(endpoint: str, token: str | None = None, **fields: object) -> tuple[str, str]:
    token = token or ergo_api_token()
    auth = f"Authorization: Bearer {token}"
    url = f"http://127.0.0.1:8089/v1/{endpoint}"
    command = ["curl", "-s", "-w", "\\n%{http_code}", "-H", auth, "-d", json.dumps(fields), url]
    body, status = credenza.succeed(shlex.join(command)).rsplit("\n", 1)
    return body, status


def register(account: str, password: str) -> str:
    body, _ = ergo_api("ns/saregister", accountName=account, passphrase=password)
    return body


def irc_command(*commands: str, host: str = "credenza", **options: str) -> str:
    flags = [f"--{name}={value}" for name, value in options.items()]
    return shlex.join(["irc-client", host, *flags, *commands])


def irc(*commands: str, machine: "QemuMachine" = client, **options: str) -> str:
    return machine.succeed(irc_command(*commands, **options))


def https(name: str, path: str = "/", *flags: str) -> str:
    address = client.succeed("getent ahostsv4 credenza | head -1 | cut -d' ' -f1").strip()
    return client.succeed(
        shlex.join(["curl", "-sk", "--resolve", f"{name}:443:{address}", *flags, f"https://{name}{path}"])
    )


members = "members.weblounge.org"
outcome = ["-o", "/dev/null", "-w", "%{http_code} %{redirect_url}"]


def concierge_admin_command(*args: str) -> str:
    return f"su - luiz -c {shlex.quote(shlex.join(['concierge-admin', *args]))}"


def concierge_admin(*args: str) -> str:
    return credenza.succeed(concierge_admin_command(*args))


def sqlite_command(database: str, query: str, *, write: bool = False) -> str:
    readonly = [] if write else ["-readonly"]
    return shlex.join([tools["sqlite3"], *readonly, database, query])


def sqlite(database: str, query: str, *, write: bool = False) -> str:
    return credenza.succeed(sqlite_command(database, query, write=write))


def concierge_sql(query: str, *, write: bool = False) -> str:
    return sqlite("/var/lib/private/concierge/concierge.db", query, write=write)


outbox = "/var/lib/concierge-outbox/outbox.db"


def outbox_sql(query: str) -> str:
    return sqlite(outbox, query)


def wait_for_empty_outbox() -> None:
    """Waits until the mailer has sent, and deleted, every queued message."""
    count = sqlite_command(outbox, "SELECT count(*) FROM outbox")
    credenza.wait_until_succeeds(f'[ "$({count})" = 0 ]', timeout=60)


def invite_code(link: str) -> str:
    return re.split(r"[#/]", link.strip())[-1]


class Browser:
    """Someone visiting members.weblounge.org from the client, with their own cookies."""

    def __init__(self, name: str) -> None:
        self.cookies = ["-b", f"/tmp/cookies-{name}", "-c", f"/tmp/cookies-{name}"]

    def get(self, path: str, *flags: str) -> str:
        return https(members, path, *self.cookies, *flags)

    def post(self, path: str, *fields: str, flags: Sequence[str] = outcome) -> str:
        """Posts a form the way concierge's own pages do."""
        data = [arg for field in fields for arg in ["--data-urlencode", field]] or ["--data", ""]
        return https(members, path, *self.cookies, "-H", f"Origin: https://{members}", *flags, *data)

    def sign_in(self, account: str, password: str) -> str:
        return self.post("/signin", f"nick={account}", f"password={password}")

    def sign_up(self, link: str, account: str, password: str, flags: Sequence[str] = outcome) -> str:
        """Fills in the invite page with the code from `link`, as its script does."""
        fields = [
            f"code={invite_code(link)}",
            f"nick={account}",
            f"email={account}@example.org",
            f"password={password}",
            f"confirmation={password}",
        ]
        return self.post("/invite", *fields, flags=flags)

    def create_invite(self) -> tuple[str, str]:
        """Answers with the page of a new invite and its link, which nothing shows again."""
        page = self.post("/invites", flags=["-f"])
        [link] = re.findall(r'<code class="invite-link">([^<]+)</code>', page)
        return page, link


def add_member(account: str, password: str, admin: bool = False) -> None:
    """Signs `account` up through a root invite, the way every member joins."""
    link = concierge_admin("invite", "create", "--root", *(["--admin"] if admin else []))
    signed_up = Browser(f"signup-{account}").sign_up(link, account, password)
    assert signed_up == "200 ", signed_up


def confirm_email(account: str) -> None:
    """Marks `account`'s email confirmed, as inviting needs, in tests without a mail node to mail the link."""
    concierge_sql(f"UPDATE members SET email_verified_at = unixepoch() WHERE account = '{account}'", write=True)


def client_addresses() -> set[str]:
    return set(credenza.succeed("getent ahosts client | cut -d' ' -f1").split())


def patterns(texts: Iterable[str]) -> str:
    return shlex.join(arg for text in texts for arg in ["-e", text])


def assert_client_address_absent(logs: str) -> None:
    credenza.succeed("journalctl --sync")
    credenza.fail(f"{logs} | grep -qF {patterns(client_addresses())}")


def files_containing(*texts: str) -> list[str]:
    """Files under /var/lib and /var/log, the journal's aside, that hold any of texts."""
    credenza.succeed("sync")
    return credenza.succeed(f"grep -rlaF --exclude-dir=journal {patterns(texts)} /var/lib /var/log || true").split()


Mail = TypedDict("Mail", {"from": str, "to": list[str], "subject": str, "text": str})


def mailpit_request(path: str, method: str = "GET") -> str:
    """Calls Mailpit's API on the mail node, for tests with `mail = true;`."""
    url = f"http://127.0.0.1:8025/api/v1/{path}"
    return mail.succeed(shlex.join(["curl", "-sf", "-X", method, url]))


def mailpit(path: str) -> Any:
    return json.loads(mailpit_request(path))


def mailbox(address: dict[str, str]) -> str:
    return f"{address['Name']} <{address['Address']}>" if address["Name"] else address["Address"]


def mails_to(address: str) -> list[Mail]:
    """The mail that reached address, oldest first."""
    summaries = mailpit(f"search?query={quote(f'to:"{address}"')}")["messages"]
    summaries.sort(key=lambda summary: summary["Created"])
    messages = [mailpit(f"message/{summary['ID']}") for summary in summaries]
    return [
        {
            "from": mailbox(message["From"]),
            "to": [mailbox(to) for to in message["To"]],
            "subject": message["Subject"],
            "text": message["Text"],
        }
        for message in messages
    ]


def wait_for_mail(to: str, subject: str | None = None, timeout: int = 60) -> Mail:
    """The newest mail to `to`, with `subject` if given, once there is one."""
    deadline = time.monotonic() + timeout
    while not (found := [message for message in mails_to(to) if subject in (None, message["subject"])]):
        assert time.monotonic() < deadline, f"no mail to {to} with subject {subject!r} in {timeout}s"
        time.sleep(1)
    return found[-1]


def link_in(message: Mail, prefix: str) -> str:
    """The one link in message's text that starts with prefix."""
    links = re.findall(rf"{re.escape(prefix)}\S*", message["text"])
    assert len(links) == 1, (prefix, message)
    return links[0]


def clear_mail() -> None:
    mailpit_request("messages", method="DELETE")


start_all()
client.wait_for_unit("multi-user.target")
