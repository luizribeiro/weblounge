import base64
import hashlib
import json
import re
import secrets
import shlex
import time
from collections.abc import Iterable, Sequence
from typing import TYPE_CHECKING, Any, TypedDict
from urllib.parse import parse_qs, quote, urlencode, urlsplit

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


def wait_for_bender(timeout: int = 60) -> None:
    """Once louro, which holds bender's connection, has it logged in and in #bots and #ops."""
    deadline = time.monotonic() + timeout
    while True:
        status, output = credenza.execute("louro status")
        louro = json.loads(output) if status == 0 else {}
        if louro.get("connected") and {"#bots", "#ops"} <= set(louro.get("channels", [])):
            return
        assert time.monotonic() < deadline, f"bender isn't in #bots and #ops after {timeout}s: {output}"
        time.sleep(1)


def history(target: str, account: str, password: str, speaker: str | None = None) -> list[str] | None:
    """What speaker, or anyone, said in target, a channel or the nick account talks to, oldest first,
    from its history as account sees it; None while IRC is down."""
    join = [f"JOIN {target}"] if target.startswith("#") else []
    commands = ["CAP REQ :batch message-tags draft/chathistory", *join, f"CHATHISTORY LATEST {target} * 100"]
    status, session = client.execute(irc_command(*commands, account=account, password=password))
    if status != 0:
        return None
    who = re.escape(speaker) if speaker else r"[^!\s]+"
    to = "|".join(re.escape(name) for name in {target, account})
    lines: dict[str, str] = {}
    for msgid, text in re.findall(rf"^@\S*?\bmsgid=([^;\s]+)\S* :{who}!\S+ PRIVMSG (?:{to}) :(.*)$", session, re.M):
        lines.setdefault(msgid, text)
    return list(lines.values())


def history_lines(target: str, account: str, password: str, speaker: str | None = None) -> list[str]:
    lines = history(target, account, password, speaker)
    assert lines is not None, "IRC is down"
    return lines


def wait_for_line(
    pattern: str,
    target: str,
    account: str,
    password: str,
    speaker: str | None = None,
    since: int = 0,
    timeout: int = 400,
) -> list[str]:
    """The lines in target from the since'th on, once one of them matches pattern, riding out a reboot."""
    deadline = time.monotonic() + timeout
    while True:
        lines = (history(target, account, password, speaker) or [])[since:]
        if any(re.fullmatch(pattern, line) for line in lines):
            return lines
        assert time.monotonic() < deadline, f"nothing in {target} matches {pattern!r} after {timeout}s: {lines}"
        time.sleep(2)


def ops_lines(account: str, password: str) -> list[str]:
    """What bender has said in #ops, oldest first."""
    return history_lines("#ops", account, password, speaker="bender")


def wait_for_ops_line(pattern: str, account: str, password: str, since: int = 0, timeout: int = 400) -> list[str]:
    return wait_for_line(pattern, "#ops", account, password, speaker="bender", since=since, timeout=timeout)


def https_command(name: str, path: str = "/", *flags: str) -> str:
    address = client.succeed("getent ahostsv4 credenza | head -1 | cut -d' ' -f1").strip()
    return shlex.join(["curl", "-sk", "--resolve", f"{name}:443:{address}", *flags, f"https://{name}{path}"])


def https(name: str, path: str = "/", *flags: str) -> str:
    return client.succeed(https_command(name, path, *flags))


members = "members.weblounge.org"


def wait_for_members(timeout: int = 300) -> None:
    """Waits until concierge answers through nginx, and Ergo's API, which signing up calls, listens.
    Their units are active before they listen."""
    client.wait_until_succeeds(https_command(members, "/signin", "-f", "-o", "/dev/null"), timeout=timeout)
    credenza.wait_for_open_port(8089, timeout=timeout)


files_host = "files.weblounge.org"
outcome = ["-o", "/dev/null", "-w", "%{http_code} %{redirect_url}"]
# The scrubber's test images, on the client.
fixtures = "/etc/upload-fixtures"

asset_types = {"css": "text/css", "svg": "image/svg+xml", "png": "image/png", "avif": "image/avif"}


def error_page(name: str, path: str, status: int) -> str:
    """The page nginx answers `path` on `name` with, once its status, its
    security headers and the files it loads, from nginx itself, check out."""
    head, page = https(name, path, "-D", "-").split("\r\n\r\n", 1)
    head = head.lower()
    assert head.split()[1] == str(status), head
    assert "x-content-type-options: nosniff" in head, head
    assert re.search(r"content-security-policy: .*style-src 'self'", head), head
    assets = set(re.findall(r'href="(/_error/assets/[^"#]+)', page))
    [stylesheet] = [asset for asset in assets if asset.endswith(".css")]
    css = https(name, stylesheet, "-f")
    felt = {f"/_error/assets/{tile}" for tile in re.findall(r'url\("\./(felt-[^"]+)"\)', css)}
    assert felt, css
    for asset in assets | felt:
        loaded = https(name, asset, "-o", "/dev/null", "-w", "%{http_code} %{content_type}")
        assert loaded == f"200 {asset_types[asset.rsplit('.', 1)[1]]}", (name, asset, loaded)
    return page


def not_found_page(name: str, path: str) -> str:
    """The landing page's not-found page nginx answers `path` on `name` with,
    linking back to the landing page whichever site it's on."""
    page = error_page(name, path, 404)
    assert '<a href="https://weblounge.org/">Back to weblounge.org →</a>' in page, page
    return page


def concierge_admin_command(*args: str) -> str:
    return f"su - luiz -c {shlex.quote(shlex.join(['concierge-admin', *args]))}"


def concierge_admin(*args: str) -> str:
    return credenza.succeed(concierge_admin_command(*args))


def sqlite_command(database: str, query: str, *, write: bool = False) -> str:
    readonly = [] if write else ["-readonly"]
    return shlex.join([tools["sqlite3"], *readonly, database, query])


def sqlite(database: str, query: str, *, write: bool = False) -> str:
    return credenza.succeed(sqlite_command(database, query, write=write))


concierge_db = "/var/lib/private/concierge/concierge.db"


def concierge_sql(query: str, *, write: bool = False) -> str:
    return sqlite(concierge_db, query, write=write)


def wait_for_concierge_sql(query: str, answer: str, timeout: int = 60) -> None:
    """Waits until query on concierge's database answers answer, without IRC's connection limits."""
    command = sqlite_command(concierge_db, query)
    credenza.wait_until_succeeds(f'[ "$({command})" = {shlex.quote(answer)} ]', timeout=timeout)


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


webchat = "https://irc.weblounge.org"


def authorize(browser, challenge, state="a-random-state"):
    """What gamja does on load: answers the code concierge sends back."""
    query = urlencode(
        {
            "response_type": "code",
            "client_id": "gamja",
            "redirect_uri": f"{webchat}/",
            "state": state,
            "code_challenge": challenge,
            "code_challenge_method": "S256",
        }
    )
    redirect = browser.get(f"/oauth/authorize?{query}", *outcome).split(" ", 1)[1]
    assert redirect.startswith(f"{webchat}/?"), redirect
    params = parse_qs(urlsplit(redirect).query)
    assert params["state"] == [state], redirect
    return params["code"][0]


def exchange(code, verifier, origin=webchat):
    fields = {
        "grant_type": "authorization_code",
        "code": code,
        "redirect_uri": f"{webchat}/",
        "client_id": "gamja",
        "code_verifier": verifier,
    }
    flags = [arg for field in fields.items() for arg in ["--data-urlencode", "=".join(field)]]
    response = https(members, "/oauth/token", "-H", f"Origin: {origin}", "-w", "\n%{http_code}", *flags)
    body, status = response.rsplit("\n", 1)
    return status, json.loads(body)


def pkce():
    verifier = secrets.token_urlsafe(32)
    challenge = base64.urlsafe_b64encode(hashlib.sha256(verifier.encode()).digest()).rstrip(b"=").decode()
    return verifier, challenge


def access_token(browser):
    verifier, challenge = pkce()
    status, answer = exchange(authorize(browser, challenge), verifier)
    assert status == "200", (status, answer)
    return answer["access_token"]


class Answer(TypedDict):
    status: int
    headers: dict[str, str]
    body: str


def answer_of(response: str) -> Answer:
    """A response curl printed with -D -, its headers' names lowercased."""
    head, body = response.split("\r\n\r\n", 1)
    status, *lines = head.splitlines()
    headers = dict(line.split(": ", 1) for line in lines)
    return {"status": int(status.split()[1]), "headers": {k.lower(): v for k, v in headers.items()}, "body": body}


def upload(path: str, *credentials: str) -> Answer:
    """Posts the file at path on the client to the upload endpoint, as an IRC
    client does, with curl's credentials flags, such as -u nick:password."""
    flags = ["-H", "Content-Type: application/octet-stream", "--data-binary", f"@{path}"]
    return answer_of(https(files_host, "/upload", "-D", "-", *credentials, *flags))


def get_file(link: str) -> Answer:
    """What files.weblounge.org answers anyone who has link."""
    path = link.removeprefix(f"https://{files_host}")
    return answer_of(https(files_host, path, "-D", "-", "-o", "/tmp/fetched", "-w", "\n"))


def add_member(account: str, password: str, admin: bool = False) -> None:
    """Signs `account` up through a root invite, the way every member joins."""
    wait_for_members()
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


def unit_file_problems(*paths: str) -> list[str]:
    """What systemd-analyze objects to in these unit files, beyond upstream units that are masked or name
    programs left out of this system."""
    _, output = credenza.execute(f"systemd-analyze verify --man=no --recursive-errors=no {' '.join(paths)} 2>&1")
    return [line for line in output.splitlines() if not re.search(r"is masked|is not executable", line)]


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
