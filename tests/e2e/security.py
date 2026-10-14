import hashlib
import re
import shlex
from html.parser import HTMLParser
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from driver import client, credenza, subtest, tools
    from helpers import Account, Browser, add_member, concierge_sql, ergo_api, https, invite_code, irc, members, outcome

mallory: Account = {"account": "mallory", "password": "an-insider-gone-bad"}
tls_handshake = "openssl s_client -cipher DEFAULT:@SECLEVEL=0 </dev/null -connect"
sites = ["weblounge.org", "members.weblounge.org", "join.weblounge.org", "irc.weblounge.org"]
pages_and_directives_they_need = {
    "weblounge.org": ("/", {"script-src", "style-src", "img-src"}),
    members: ("/signin", {"script-src", "style-src", "img-src"}),
    "irc.weblounge.org": ("/", {"script-src", "style-src", "img-src", "manifest-src"}),
}


def assert_loopback_only(port, path):
    listeners = credenza.succeed(f"ss -Htln 'sport = :{port}'")
    assert listeners.split()[3] == f"127.0.0.1:{port}" and listeners.count("\n") == 1, listeners
    client.fail(f"curl -s --max-time 3 http://credenza:{port}{path}")


def headers_of(name, path):
    response = https(name, path, "-I").lower()
    return dict(line.split(": ", 1) for line in response.strip().splitlines()[1:])


def csp_sources(policy, directive):
    directives = dict((part.split(None, 1) + [""])[:2] for part in policy.split(";") if part.strip())
    return directives.get(directive, directives.get("default-src", "")).split()


class PageNeeds(HTMLParser):
    """What a page asks the browser to run inline, and what it fetches under which CSP directive."""

    directives = {
        "script": "script-src",
        "img": "img-src",
        "stylesheet": "style-src",
        "icon": "img-src",
        "manifest": "manifest-src",
    }

    def __init__(self, page):
        super().__init__()
        self.inline = []
        self.fetches = []
        self.feed(page)

    def handle_starttag(self, tag, attrs):
        attrs = dict(attrs)
        if (
            tag == "style"
            or (tag == "script" and "src" not in attrs)
            or any(name == "style" or name.startswith("on") for name in attrs)
        ):
            self.inline.append((tag, attrs))
        url = attrs.get("src") or (tag == "link" and attrs.get("href"))
        if url:
            kind = attrs.get("rel") if tag == "link" else tag
            self.fetches.append((self.directives.get(kind or ""), url))


credenza.wait_for_unit("nginx.service")
credenza.wait_for_unit("concierge.service")
credenza.wait_for_open_port(8080)
credenza.wait_for_unit("ergochat.service")
credenza.wait_for_open_port(6697)

with subtest("only SSH, HTTP, HTTPS and IRC over TLS are reachable, even with a stray listener up"):
    credenza.succeed(f"systemd-run --unit=stray-listener -- {tools['python3']} -m http.server 9000")
    credenza.wait_for_open_port(9000)
    scan = client.succeed("nmap -Pn -n -p- --min-rate 20000 --max-retries 1 --open -oG - credenza")
    assert set(re.findall(r"(\d+)/open/tcp", scan)) == {"22", "80", "443", "6697"}, scan

with subtest("there's no plaintext IRC"):
    credenza.fail("ss -Htln | grep -q ':6667 '")
    client.fail("nc -z -w 3 credenza 6667")

with subtest("HTTPS and IRC refuse TLS older than 1.2"):
    for port in [443, 6697]:
        for version in ["tls1", "tls1_1"]:
            _, output = client.execute(f"{tls_handshake} credenza:{port} -{version} 2>&1")
            assert "alert protocol version" in output, (port, version, output)
        for version in ["tls1_2", "tls1_3"]:
            client.succeed(f"{tls_handshake} credenza:{port} -{version}")

with subtest("ergo's API and WebSocket listener and concierge listen on credenza's loopback only"):
    assert_loopback_only(8089, "/v1/status")
    assert_loopback_only(8097, "/")
    assert_loopback_only(8080, "/healthz")

with subtest("concierge's introspection answers Ergo's secret alone"):
    introspect = "curl -s -o /dev/null -w '%{{http_code}}' -d token=x -u {} http://127.0.0.1:8080/oauth/introspect"
    secret = credenza.succeed("cat /var/lib/weblounge-secrets/oauth-introspection-secret").strip()
    for credentials in ["ergo:wrong", "ergo:4TA0I7mJ3fUUcW05KJiODg", f"gamja:{secret}"]:
        assert credenza.succeed(introspect.format(shlex.quote(credentials))) == "401", credentials
    assert credenza.succeed(introspect.format(shlex.quote(f"ergo:{secret}"))) == "200"

with subtest("introspection isn't reachable from outside, and the token endpoint takes the web client's origin alone"):
    assert https(members, "/oauth/introspect", "-o", "/dev/null", "-w", "%{http_code}", "-d", "token=x") == "404"
    for origin in ["https://evil.example", f"https://{members}"]:
        response = https(
            members, "/oauth/token", "-i", "-H", f"Origin: {origin}", "-d", "grant_type=authorization_code"
        )
        assert response.startswith("HTTP/2 403"), (origin, response)
        assert "access-control-allow-origin" not in response.lower(), (origin, response)

with subtest("ergo's API refuses any token but the generated one"):
    for token in ["not-the-token", "example"]:
        _, status = ergo_api("status", token=token)
        assert status == "401", (token, status)

with subtest("the generated secrets stay out of the Nix store, readable by root alone"):
    secrets = "/var/lib/weblounge-secrets"
    names = ["ergo-api-token", "session-key", "oauth-introspection-secret"]
    modes = credenza.succeed(f"cd {secrets} && stat -c '%a %U' . {' '.join(names)}")
    assert modes == "700 root\n" + "600 root\n" * len(names), modes
    ergo_config = (
        "systemctl cat ergochat; cat /etc/ergo.yaml "
        "$(systemctl show -P ExecStart ergochat | grep -o '/nix/store/[^ ;]*')"
    )
    concierge_config = "systemctl cat concierge; cat $(readlink -f $(command -v concierge-admin))"
    for secret, config in [
        ("ergo-api-token", ergo_config),
        ("session-key", concierge_config),
        ("oauth-introspection-secret", f"{ergo_config}; {concierge_config}"),
    ]:
        credenza.fail(f"{{ {config}; }} | grep -qF $(cat {secrets}/{secret})")

with subtest("default.yaml's example oper and WebIRC gateway, with published credentials, are left out"):
    credenza.fail("grep -qE '^opers:|webirc' /etc/ergo.yaml")

with subtest("ergo, concierge, nginx and bender run sandboxed"):
    for unit in ["ergochat", "concierge", "nginx", "weblounge-bot-bender"]:
        credenza.succeed(f"systemd-analyze security --threshold=20 --no-pager {unit}.service")

with subtest("a client that doesn't log in with SASL is turned away, even from loopback"):
    for reply in [irc(), irc(machine=credenza, host="localhost")]:
        assert " 001 " not in reply and re.search(r"^ERROR .*SASL", reply, re.M), reply

with subtest("NickServ won't register new accounts, even for a member"):
    add_member(**mallory)
    reply = irc("PRIVMSG NickServ :REGISTER battery-staple", **mallory)
    assert "NOTICE mallory :This command has been disabled" in reply, reply

with subtest("every site sends HSTS, nosniff, a referrer policy and a CSP that forbids framing"):
    for name in sites:
        for path in ["/", "/no-such-page"]:
            headers = headers_of(name, path)
            hsts = headers.get("strict-transport-security", "")
            max_age = re.search(r"max-age=(\d+)", hsts)
            assert max_age and int(max_age[1]) >= 365 * 24 * 3600, (name, path, hsts)
            assert "includesubdomains" in hsts and "preload" not in hsts, (name, path, hsts)
            assert headers.get("x-content-type-options") == "nosniff", (name, path, headers)
            assert headers.get("referrer-policy") == "same-origin", (name, path, headers)
            assert "frame-ancestors 'none'" in headers.get("content-security-policy", ""), (name, path, headers)

with subtest("the landing page, concierge and gamja load all they need under their CSP, and nothing inline"):
    for name, (path, directives) in pages_and_directives_they_need.items():
        policy = headers_of(name, path)["content-security-policy"]
        needs = PageNeeds(https(name, path, "-f"))
        assert needs.inline == [], (name, needs.inline)
        assert {directive for directive, _ in needs.fetches} == directives, (name, needs.fetches)
        for directive, url in needs.fetches:
            same_origin = not re.match(r"[a-z]+:|//", url)
            assert same_origin and csp_sources(policy, directive) == ["'self'"], (name, directive, url, policy)

with subtest("concierge's pages fetch from their own origin alone, for the checks they make as you type"):
    policy = headers_of(members, "/signin")["content-security-policy"]
    assert csp_sources(policy, "connect-src") == ["'self'"], policy


def sign_in(account, password, *headers):
    flags = [arg for header in headers for arg in ["-H", header]]
    fields = ["--data-urlencode", f"nick={account}", "--data-urlencode", f"password={password}"]
    return https(members, "/signin", *outcome, *flags, *fields)


def sign_in_from_our_pages(account, password, *headers):
    return sign_in(account, password, f"Origin: https://{members}", *headers)


with subtest("signed-out visitors to members' pages are sent to sign in"):
    assert https(members, "/", *outcome) == f"303 https://{members}/signin"
    assert https(members, "/?saved", *outcome) == f"303 https://{members}/signin?next=%2F%3Fsaved"

with subtest("the session cookie is host-only, Secure, HttpOnly and SameSite=Lax"):
    response = https(
        members,
        "/signin",
        "-D",
        "-",
        "-o",
        "/dev/null",
        "-H",
        f"Origin: https://{members}",
        "--data-urlencode",
        "nick=mallory",
        "--data-urlencode",
        f"password={mallory['password']}",
    )
    [cookie] = [line for line in response.splitlines() if line.lower().startswith("set-cookie:")]
    attributes = {part.strip().split("=")[0].lower() for part in cookie.split(";")[1:]}
    assert cookie.split(":", 1)[1].strip().startswith("__Host-session="), cookie
    assert {"path", "httponly", "secure", "samesite"} <= attributes and "domain" not in attributes, cookie
    assert "SameSite=Lax" in cookie and "Path=/;" in cookie, cookie

with subtest("a password change from another origin is refused, even with the member's session"):
    mallorys_browser = Browser("mallory")
    assert mallorys_browser.sign_in(**mallory).startswith("303 ")
    fields = [f"current={mallory['password']}", "password=chosen-by-evil-1", "confirmation=chosen-by-evil-1"]
    refused = https(
        members,
        "/password",
        *mallorys_browser.cookies,
        "-H",
        "Origin: https://evil.example",
        *outcome,
        *[arg for field in fields for arg in ["--data-urlencode", field]],
    )
    assert refused == "403 ", refused
    body, _ = ergo_api("check_auth", accountName="mallory", passphrase=mallory["password"])
    assert '"success":true' in body, body

with subtest("a new invite's page draws its QR code within the CSP, and nothing shows its code again"):
    page, link = mallorys_browser.create_invite()
    code = link.rsplit("/", 1)[1]
    needs = PageNeeds(page)
    assert needs.inline == [] and all(directive for directive, _ in needs.fetches), (needs.inline, needs.fetches)
    assert "<svg" in page, page
    assert code not in mallorys_browser.get("/invites", "-f")
    credenza.succeed("journalctl --sync")
    credenza.fail(f"grep -qF {code} /var/log/nginx/access.log")
    credenza.fail(f"journalctl | grep -qF {code}")

with subtest("a POST from another origin, or from nowhere, is refused"):
    for headers in [["-H", "Origin: https://evil.example"], ["-H", "Referer: https://evil.example/"], []]:
        status = https(members, "/signin", "-o", "/dev/null", "-w", "%{http_code}", "--data", "", *headers)
        assert status == "403", (headers, status)

# The limits are concierge's rate_limit::PER_ACCOUNT and PER_ADDRESS.
per_account, per_address = 5, 20

with subtest("wrong passwords for one account get it rate-limited, even with the right one"):
    for _ in range(per_account):
        assert sign_in_from_our_pages("mallory", "guess") == "401 "
    limited = sign_in_from_our_pages("mallory", mallory["password"])
    assert limited == "429 ", limited

with subtest("forging X-Forwarded-For or X-Real-IP doesn't get around the per-address limit"):
    for n in range(per_address - per_account):
        forged = [f"X-Forwarded-For: 203.0.113.{n}", f"X-Real-IP: 198.51.100.{n}"]
        assert sign_in_from_our_pages(f"guess{n}", "guess", *forged) == "401 ", n
    limited = sign_in_from_our_pages("someone", "guess", "X-Real-IP: 192.0.2.1", "X-Forwarded-For: 192.0.2.1")
    assert limited == "429 ", limited


def mint_invite():
    return credenza.succeed("su - luiz -c 'concierge-admin invite create --root'").strip()


def sign_up(link, nick, origin, flags=("-o", "/dev/null", "-w", "%{http_code}")):
    fields = [
        f"code={invite_code(link)}",
        f"nick={nick}",
        f"email={nick}@example.org",
        "password=lantern-orchard-71",
        "confirmation=lantern-orchard-71",
    ]
    return https(
        members,
        "/invite",
        "-H",
        f"Origin: {origin}",
        *flags,
        *[arg for field in fields for arg in ["--data-urlencode", field]],
    )


with subtest("opening an invite link and signing up leaves the code out of nginx's log and the journal"):
    link = mint_invite()
    code = invite_code(link)
    https(members, "/invite", "-f", "-o", "/dev/null")
    https(
        members,
        "/signup/invite",
        "-f",
        "-o",
        "/dev/null",
        "-H",
        f"Origin: https://{members}",
        "--data-urlencode",
        f"code={code}",
    )
    assert sign_up(link, "walter", f"https://{members}") == "200"
    access_log = credenza.succeed("cat /var/log/nginx/access.log")
    assert re.search(r'"POST /invite HTTP/[\d.]+" 200', access_log), access_log
    assert code not in access_log, access_log
    credenza.succeed("journalctl --sync")
    credenza.fail(f"journalctl | grep -qF {code}")

with subtest("a signup posted from another origin is refused, and the invite still works"):
    link = mint_invite()
    assert sign_up(link, "eve", "https://evil.example") == "403"
    assert sign_up(link, "eve", f"https://{members}") == "200"

with subtest("used, expired and unknown codes all get the same answer"):
    used, expired = mint_invite(), mint_invite()
    assert sign_up(used, "trudy", f"https://{members}") == "200"
    code_hash = hashlib.sha256(invite_code(expired).encode()).hexdigest()
    concierge_sql(f"UPDATE invites SET created_at = 0, expires_at = 1 WHERE code_hash = x'{code_hash}'", write=True)
    answers = [
        sign_up(link, "mallet", f"https://{members}", flags=("-w", "\n%{http_code}"))
        for link in [used, expired, "q7Hx2mZp9vLcT4wRfBa-_w"]
    ]
    assert all(answer.endswith("\n422") for answer in answers), answers
    errors = {error for answer in answers for error in re.findall(r'id="code-hint">([^<]*)<', answer)}
    assert len(errors) == 1 and "This invite doesn&#39;t work." in errors.pop(), answers

# concierge's rate_limit::LOOKUPS_PER_ADDRESS.
lookups_per_address = 60

with subtest("nick lookups are rate-limited per address, whatever X-Real-IP says"):

    def lookup(n):
        return https(
            members,
            "/signup/nick",
            "-o",
            "/dev/null",
            "-w",
            "%{http_code}",
            "-H",
            f"Origin: https://{members}",
            "-H",
            f"X-Real-IP: 198.51.100.{n % 250}",
            "--data",
            "nick=bob",
        )

    # mallory's, walter's, eve's and trudy's signups above counted as one
    # each, and so did the lookup of walter's code.
    allowed = lookups_per_address - 5
    statuses = [lookup(n) for n in range(allowed + 1)]
    assert statuses == ["200"] * allowed + ["429"], statuses
