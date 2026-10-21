import html
import json
import re
import shlex
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from driver import client, credenza, subtest
    from helpers import (
        Account,
        Browser,
        add_member,
        concierge_admin,
        concierge_admin_command,
        concierge_sql,
        confirm_email,
        ergo_api,
        error_page,
        https,
        irc,
        irc_command,
        members,
        not_found_page,
        outcome,
    )

with subtest("members.weblounge.org serves concierge's sign-in page in the shared styles"):
    credenza.wait_for_unit("nginx.service")
    credenza.wait_for_unit("concierge.service")
    credenza.wait_for_open_port(8080)
    page = https("members.weblounge.org", "/signin", "-f")
    assert "Sign in." in page, page
    stylesheet = re.search(r'href="(/assets/members-[A-Z0-9]+\.css)"', page)
    assert stylesheet, page
    css = https("members.weblounge.org", stylesheet[1], "-f")
    assert "--paper:" in css, css

with subtest("concierge serves the assets its page links, under hashed names cached for good"):
    member_assets = set(re.findall(r'(?:href|src)="(/assets/[^"#]+)', page))
    for asset in member_assets:
        assert re.fullmatch(r"/assets/\w+-[A-Z0-9]{8}\.\w+", asset), asset
        headers = https("members.weblounge.org", asset, "-fI").lower()
        assert "cache-control: public, max-age=31536000, immutable" in headers, headers
    types = {
        https("members.weblounge.org", asset, "-f", "-o", "/dev/null", "-w", "%{content_type}")
        for asset in member_assets
    }
    assert types == {"text/css", "text/javascript", "image/svg+xml"}, (member_assets, types)

with subtest("the header draws the mark and wordmark from files that have the ids it uses"):
    for asset, fragment in re.findall(r'<use href="(/assets/[^"#]+)#([^"]+)"', page):
        svg = https("members.weblounge.org", asset, "-f")
        assert f'id="{fragment}"' in svg, (asset, svg)

with subtest("concierge answers health checks"):
    health = https("members.weblounge.org", "/healthz", "-f")
    assert health == "ok", health

with subtest("join.weblounge.org sends invite links on to the invite page, and nothing else anywhere"):
    assert https("join.weblounge.org", "/", *outcome) == f"302 https://{members}/invite"
    not_found_page("join.weblounge.org", "/invite")

with subtest("concierge answers unknown paths with its own not-found page"):
    lost = https(members, "/no-such-page", "-w", "\n%{http_code}")
    assert lost.endswith("\n404") and "<h1>There&#39;s nothing here.</h1>" in lost, lost
    assert re.search(r'href="/assets/members-[A-Z0-9]+\.css"', lost), lost

with subtest("introspection stays nginx's bare 404 from outside, error pages or not"):
    for flags in [[], ["-d", "token=x"]]:
        response = https(members, "/oauth/introspect", "-i", *flags)
        assert response.startswith("HTTP/2 404") and "<center>nginx</center>" in response, response
        assert "_error" not in response, response

with subtest("while concierge is down, nginx says it's back in a moment, and it's back once it starts"):
    credenza.systemctl("stop concierge.service")
    down = error_page(members, "/account?saved", 502)
    assert "<h1>Back in a moment.</h1>" in down, down
    assert '<a class="button button-primary" href="">Try again</a>' in down, down
    assert "<script" not in down and "Sign in" not in down, down
    credenza.systemctl("start concierge.service")
    credenza.wait_for_open_port(8080)
    assert "Sign in." in https(members, "/signin", "-f")

with subtest("concierge's release build carries no live-reload code"):
    assert "livereload" not in page, page
    credenza.fail("grep -aq tower-livereload /proc/$(systemctl show -P MainPID concierge)/exe")

with subtest("concierge-admin runs concierge as the service does, with its state, secrets, database and Ergo"):
    service_uid = credenza.succeed("ps -o uid= -p $(systemctl show -P MainPID concierge)").strip()
    assert service_uid != "0", service_uid
    credenza.wait_for_open_port(8089)
    status = concierge_admin("status")
    assert status == (
        "public url: https://members.weblounge.org\n"
        f"state directory: /var/lib/concierge (writable by uid {service_uid})\n"
        "session key: 64 bytes\n"
        "database: /var/lib/concierge/concierge.db (migration 20261030120000, 0 members)\n"
        "ergo: http://127.0.0.1:8089/ answers, version 2.19.1\n"
    ), (service_uid, status)
    credenza.fail(concierge_admin_command("no-such-command"))

with subtest("concierge-migrate brings both databases up to date before concierge or its mailer open them"):
    migrated = credenza.succeed("journalctl -o cat -u concierge-migrate")
    assert "/var/lib/concierge/concierge.db is at migration 20261030120000" in migrated, migrated
    assert "/var/lib/concierge-outbox/outbox.db is at migration 20261028120000" in migrated, migrated
    credenza.succeed("systemctl stop concierge-migrate")
    stopped = credenza.succeed("systemctl is-active concierge concierge-mailer || true").split()
    assert stopped == ["inactive", "inactive"], stopped
    credenza.succeed("systemctl start concierge concierge-mailer")
    credenza.succeed("systemctl is-active concierge-migrate")
    credenza.wait_for_open_port(8080)

with subtest("the outbox is a database of its own, in a directory only concierge and its mailer may write"):
    credenza.wait_for_unit("concierge-mailer.service")
    concierge_admin("mail", "test", "dana@example.org")
    modes = credenza.succeed("cd /var/lib/concierge-outbox && stat -c '%n %a %G' . outbox.db*").splitlines()
    assert modes == [
        ". 2770 concierge-outbox",
        "outbox.db 660 concierge-outbox",
        "outbox.db-shm 660 concierge-outbox",
        "outbox.db-wal 660 concierge-outbox",
    ], modes
    for unit, groups in {
        "concierge": ["concierge-outbox", "concierge-files", "concierge-scrubber-socket"],
        "concierge-mailer": ["concierge-outbox"],
    }.items():
        joined = credenza.succeed(f"systemctl show -P SupplementaryGroups {unit}").split()
        assert joined == groups, (unit, joined)
    tables = concierge_sql("SELECT name FROM sqlite_schema WHERE type = 'table'").split()
    assert "outbox" not in tables and "members" in tables, tables
    credenza.succeed("systemctl start concierge-mailer-vacuum")

with subtest("the hourly vacuums of both databases wait for concierge-migrate"):
    for unit in ["concierge-vacuum", "concierge-mailer-vacuum"]:
        after = credenza.succeed(f"systemctl show -P After {unit}").split()
        assert "concierge-migrate.service" in after, (unit, after)

with subtest("a vacuum leaves a missing database missing, and warns that it is"):
    credenza.succeed("systemctl stop concierge concierge-mailer")
    credenza.succeed("mkdir /root/outbox && mv /var/lib/concierge-outbox/outbox.db* /root/outbox/")
    credenza.succeed("systemctl start concierge-mailer-vacuum")
    credenza.succeed("test ! -e /var/lib/concierge-outbox/outbox.db")
    warnings = credenza.succeed("journalctl -o cat -p warning -u concierge-mailer-vacuum")
    assert "/var/lib/concierge-outbox/outbox.db is missing" in warnings, warnings
    credenza.succeed("mv /root/outbox/* /var/lib/concierge-outbox/ && rmdir /root/outbox")
    credenza.succeed("systemctl start concierge concierge-mailer")
    credenza.wait_for_open_port(8080)

with subtest("concierge's database lives in its state directory, private to the service"):
    modes = credenza.succeed(
        "stat -L -c %a /var/lib/private /var/lib/concierge /var/lib/concierge/concierge.db"
    ).split()
    assert modes == ["700", "700", "600"], modes

with subtest("concierge-admin invite create --root prints a link to join.weblounge.org"):
    link = concierge_admin("invite", "create", "--root")
    assert re.fullmatch(r"https://join\.weblounge\.org/#[A-Za-z0-9_-]{22}\n", link), link

with subtest("an admin's root invite brings the network's first member in"):
    credenza.wait_for_open_port(8089)
    dana: Account = {"account": "Dana", "password": "an-old-password-of-hers"}
    add_member(**dana, admin=True)
    status = concierge_admin("status")
    assert "(migration 20261030120000, 1 members)" in status, status

with subtest("concierge-admin member reset-password replaces the IRC password and prints the new one"):
    password = concierge_admin("member", "reset-password", "dana").strip()
    assert re.fullmatch("[a-z2-9]{26}", password), password
    client.fail(irc_command(**dana))
    session = irc(account="Dana", password=password)
    assert " 903 " in session, session

browser = Browser("dana")

with subtest("a member signs in to the website with their IRC password"):
    signed_in = browser.sign_in("dana", password)
    assert signed_in == f"303 https://{members}/", signed_in
    home = browser.get("/", "-f")
    assert "Pull up a chair, Dana." in home, home
    assert home.count("<circle ") == 53, home
    clients = browser.get("/clients", "-f")
    assert "<code>irc.weblounge.org</code>" in clients and "<code>6697</code>" in clients, clients

with subtest("a member reads the Help tab and its articles, which guests are sent to sign in for"):
    index = browser.get("/help", "-f")
    assert "<h1>How can we help?</h1>" in index, index
    assert '<li><a href="/help/make-a-bot">Make a bot</a></li>' in index, index
    article = browser.get("/help/make-a-bot", "-f")
    assert "<h1>Make a bot</h1>" in article, article
    assert "<code>weblounge/bot/Dana</code>" in article, article
    guest = https(members, "/help/make-a-bot", *outcome)
    assert guest.startswith(f"303 https://{members}/signin"), guest

with subtest("changing the email needs the member's password: a wrong one changes nothing"):
    fields = ["email=dana@example.org", "current=a-wrong-guess"]
    refused = browser.post("/settings", *fields, flags=["-w", "\n%{http_code}"])
    assert refused.endswith("\n401"), refused
    assert '<p class="field-error" id="email-password-hint">Wrong password.</p>' in refused, refused
    assert concierge_sql("SELECT email FROM members WHERE account = 'Dana'").strip() != "dana@example.org"

with subtest("a member changes their email and news setting on the Account tab"):
    saved = browser.post("/settings", "email=dana@example.org", "news=on", f"current={password}")
    assert saved == f"303 https://{members}/account?saved&sent#email-and-news", saved
    tab = browser.get("/account?saved", "-f")
    assert "Saved." in tab and 'value="dana@example.org"' in tab, tab
    assert 'type="checkbox" checked' in tab, tab

with subtest("a member changes their password: IRC takes only the new one, and their other sessions end"):
    laptop = Browser("dana-laptop")
    assert laptop.sign_in("dana", password).startswith("303 ")
    old_password, password = password, "a-passphrase-dana-chose"
    changed = browser.post("/password", f"current={old_password}", f"password={password}", f"confirmation={password}")
    assert changed == f"303 https://{members}/account?password#change-password", changed
    assert "Password changed." in browser.get("/account?password", "-f")
    after = laptop.get("/", *outcome)
    assert after == f"303 https://{members}/signin", after
    client.fail(irc_command(account="Dana", password=old_password))
    session = irc(account="Dana", password=password)
    assert " 903 " in session, session

with subtest("a member turns always-on off on the Account tab, and Ergo has it"):
    saved = browser.post("/irc-settings", "auto_away=on")
    assert saved == f"303 https://{members}/account?irc#irc-settings", saved
    body, _ = ergo_api("ns/saget", accountName="Dana")
    settings = json.loads(body)
    assert (settings["alwaysOn"], settings["autoAway"]) == ("off", "default"), body
    tab = browser.get("/account?irc", "-f")
    assert '<input name="always_on" type="checkbox">' in tab, tab
    assert '<input name="auto_away" type="checkbox" checked>' in tab, tab
    assert "Both on by default." in tab, tab

with subtest("in a browser, the Account tab's Save buttons and the email's password field follow changes"):
    result = client.succeed(f"account-browser dana {shlex.quote(password)} 2>&1")
    assert "save buttons and the email's password field follow changes" in result, result

with subtest("the invite tree lists a new member under whoever invited them"):
    confirm_email("Dana")
    _, link = browser.create_invite()
    assert Browser("ivy").sign_up(link, "ivy", "ivy-chose-this-one") == "200 "
    tree = browser.get("/invites", "-f")
    assert re.search(r'aria-current="true">Dana</strong>.*?<ul>\s*<li>\s*<span>ivy</span>', tree, re.S), tree

with subtest("revoking a member's open invites ends their links and frees their places"):
    _, link = browser.create_invite()
    assert "Your oldest unused invite expires in 24 hours." in browser.get("/invites", "-f")
    assert browser.post("/invites/revoke") == "303 https://members.weblounge.org/invites"
    assert Browser("jo").sign_up(link, "jo", "jo-never-got-in") == "422 "
    assert "oldest unused" not in browser.get("/invites", "-f")

with subtest("in a browser, at a phone's width and a desktop's, Revoke all asks in a dialog first, then revokes"):
    result = client.succeed(f"invites-browser dana {shlex.quote(password)} 2>&1")
    assert "revoking fits and works at both widths" in result, result
    client.copy_from_vm("/tmp/screens")

with subtest("WeeChat's certificate guide's commands attach a certificate that SASL EXTERNAL logs in with"):
    guide = browser.get("/clients/weechat/certificate", "-f")
    generate, fingerprint = [
        html.unescape(block) for block in re.findall(r"<pre><code>(.*?)</code></pre>", guide, re.S)[:2]
    ]
    assert "/CN=Dana" in generate, generate
    client.succeed(f"mkdir -p /tmp/cert && cd /tmp/cert && {generate}")
    certfp = client.succeed(f"cd /tmp/cert && {fingerprint}").strip()
    assert re.fullmatch("[0-9a-f]{64}", certfp), certfp
    added = irc(f"PRIVMSG NickServ :CERT ADD {certfp}", account="Dana", password=password)
    assert "NOTICE Dana :Certificate fingerprint successfully added" in added, added
    client.succeed("cat /tmp/cert/weblounge.key /tmp/cert/weblounge.crt > /tmp/cert/weblounge.pem")
    session = irc(account="Dana", cert="/tmp/cert/weblounge.pem")
    assert " 903 " in session and re.search(r" 900 \S+ .* :You are now logged in as Dana", session), session

with subtest("signing out ends the session"):
    signed_out = browser.post("/signout")
    assert signed_out == f"303 https://{members}/signin", signed_out
    after = browser.get("/", *outcome)
    assert after == f"303 https://{members}/signin", after

with subtest("concierge-admin member reset-password ends the member's website sessions"):
    assert browser.sign_in("dana", password).startswith("303 ")
    assert browser.get("/", *outcome).startswith("200 ")
    new_password = concierge_admin("member", "reset-password", "dana").strip()
    after = browser.get("/", *outcome)
    assert after == f"303 https://{members}/signin", after
    assert browser.sign_in("dana", new_password).startswith("303 ")

with subtest("a bot made with concierge-admin logs in to IRC with SASL, but not to the members' site"):
    bot_password = concierge_admin("bot", "create", "dana", "bender", "--note", "answers !ping").strip()
    assert re.fullmatch("[a-z2-9]{26}", bot_password), bot_password
    listed = concierge_admin("bot", "list", "dana")
    assert re.fullmatch(r"bender Dana \d{4}-\d\d-\d\d answers !ping\n", listed), listed
    session = irc(account="bender", password=bot_password)
    assert " 903 " in session, session
    body, _ = ergo_api("ns/saget", accountName="bender")
    settings = json.loads(body)
    assert (settings["alwaysOn"], settings["autoAway"]) == ("off", "off"), body
    refused = Browser("bender").sign_in("bender", bot_password)
    assert refused == "401 ", refused
    tab = browser.get("/bots", "-f")
    assert "<strong>bender</strong>" in tab and "answers !ping" in tab, tab

with subtest("concierge-admin bot delete erases the bot's account in Ergo"):
    deleted = concierge_admin("bot", "delete", "bender")
    assert deleted == "removed bot bender\n", deleted
    client.fail(irc_command(account="bender", password=bot_password))
    body, _ = ergo_api("ns/info", accountName="bender")
    assert json.loads(body) == {"success": False}, body
    assert concierge_admin("bot", "list") == ""

with subtest("the passwords never reach the journal"):
    credenza.succeed("journalctl --sync")
    for secret in [old_password, password, new_password, bot_password]:
        credenza.fail(f"journalctl | grep -qF {secret}")
