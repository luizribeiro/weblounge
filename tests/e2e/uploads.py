import re
import shlex
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from driver import client, credenza, subtest
    from helpers import (
        Account,
        Answer,
        Browser,
        access_token,
        add_member,
        concierge_admin,
        concierge_sql,
        files_host,
        fixtures,
        get_file,
        https,
        upload,
    )

zoe: Account = {"account": "zoe", "password": "a-photo-of-the-harbour"}
yuki: Account = {"account": "yuki", "password": "snow-on-the-lantern-roof"}
uploads_dir = "/var/lib/concierge-files"
link_pattern = rf"https://{re.escape(files_host)}/[A-Za-z0-9_-]{{22}}\.(png|jpg|gif|webp)"


def login(account: Account) -> str:
    return f"{account['account']}:{account['password']}"


def stored(answer: Answer, extension: str) -> str:
    """The link a successful upload answers, once its file is in place."""
    assert answer["status"] == 201, answer
    link = answer["headers"]["location"]
    assert re.fullmatch(link_pattern, link) and link.endswith(f".{extension}"), link
    assert answer["body"] == f"{link}\n", answer
    file = link.rsplit("/", 1)[1]
    credenza.succeed(f"test -f {uploads_dir}/{file}")
    return link


def stored_files() -> list[str]:
    return credenza.succeed(f"ls -A {uploads_dir}").split()


credenza.wait_for_unit("nginx.service")
credenza.wait_for_unit("concierge.service")
credenza.wait_for_open_port(8080)
credenza.wait_for_unit("ergochat.service")
credenza.wait_for_open_port(8089)

add_member(**zoe)
zoes_browser = Browser("zoe")
assert zoes_browser.sign_in(**zoe).startswith("303 ")
zoebot: Account = {"account": "zoebot", "password": concierge_admin("bot", "create", "zoe", "zoebot").strip()}

with subtest("the web client's token uploads a photo, and its link serves it to anyone, scrubbed, as an image"):
    token = access_token(zoes_browser)
    photo = stored(upload(f"{fixtures}/photo.jpg", "-H", f"Authorization: Bearer {token}"), "jpg")
    served = get_file(photo)
    assert served["status"] == 200, served
    headers = served["headers"]
    for name, value in {
        "content-type": "image/jpeg",
        "content-disposition": "inline",
        "x-content-type-options": "nosniff",
        "content-security-policy": "default-src 'none'; frame-ancestors 'none'; sandbox",
        "cross-origin-resource-policy": "cross-origin",
        "referrer-policy": "no-referrer",
        "x-robots-tag": "noindex, nofollow, noimageindex, noai, noimageai",
    }.items():
        assert headers.get(name) == value, (name, headers)
    assert "max-age=31536000" in headers["strict-transport-security"], headers
    client.succeed("head -c 3 /tmp/fetched | od -An -tx1 | grep -q 'ff d8 ff'")
    client.fail("grep -qaF Pixelphone /tmp/fetched")

with subtest("a member's and a bot's IRC logins upload, each image under the type it is"):
    stored(upload(f"{fixtures}/photo.png", "-u", login(zoe)), "png")
    stored(upload(f"{fixtures}/animated.gif", "-u", login(zoebot)), "gif")
    stored(upload(f"{fixtures}/animated.webp", "-u", login(zoebot)), "webp")
    owners = concierge_sql("SELECT DISTINCT owner FROM uploads").split()
    assert owners == ["zoe"], owners

with subtest("wrong credentials get 401, and leave nothing"):
    before = stored_files()
    for credentials in [["-u", "zoe:not-her-password"], ["-H", "Authorization: Bearer not-a-token"], []]:
        answer = upload(f"{fixtures}/photo.png", *credentials)
        assert answer["status"] == 401, (credentials, answer)
    assert stored_files() == before

with subtest("an image past 10 MB gets 413 with the limit, as senpai reads it"):
    client.succeed("head -c 10000001 /dev/zero > /tmp/too-big")
    answer = upload("/tmp/too-big", "-u", login(zoe))
    assert answer["status"] == 413 and answer["headers"]["upload-limit"] == "maxsize=10000000", answer

with subtest("SVG, HTML and PDF get 415, whatever they're called"):
    for name, content in [
        ("drawing.svg", '<svg xmlns="http://www.w3.org/2000/svg" onload="alert(1)"/>'),
        ("page.html", "<!doctype html><script>alert(document.domain)</script>"),
        ("paper.pdf", "%PDF-1.7\n%%EOF\n"),
    ]:
        client.succeed(f"printf %s {shlex.quote(content)} > /tmp/{name}")
        answer = upload(f"/tmp/{name}", "-u", login(zoe), "-H", "Content-Type: image/png")
        assert answer["status"] == 415, (name, answer)

with subtest("files.weblounge.org asks robots to stay away"):
    robots = https(files_host, "/robots.txt", "-D", "-")
    assert robots.endswith("\r\n\r\nUser-agent: *\nDisallow: /\n"), robots
    assert "x-robots-tag: noindex, nofollow, noimageindex, noai, noimageai" in robots.lower(), robots

with subtest("deleting a bot deletes its uploads, and deleting a member deletes theirs"):
    zoebots = concierge_sql("SELECT id FROM uploads WHERE account = 'zoebot'").split()
    assert len(zoebots) == 2, zoebots
    concierge_admin("bot", "delete", "zoebot")
    for upload_id in zoebots:
        assert not any(name.startswith(upload_id) for name in stored_files()), upload_id
    assert concierge_sql("SELECT count(*) FROM uploads WHERE account = 'zoebot'").strip() == "0"
    assert get_file(photo)["status"] == 200
    concierge_admin("member", "delete", "zoe")
    assert get_file(photo)["status"] == 404
    assert stored_files() == [] and concierge_sql("SELECT count(*) FROM uploads").strip() == "0"

with subtest("an upload is deleted once its 30 days are up, by the hourly timer"):
    add_member(**yuki)
    link = stored(upload(f"{fixtures}/still.webp", "-u", login(yuki)), "webp")
    credenza.succeed("systemctl list-timers concierge-expire-uploads.timer | grep -q concierge-expire-uploads")
    concierge_sql("UPDATE uploads SET created_at = created_at - 29 * 86400", write=True)
    credenza.succeed("systemctl start concierge-expire-uploads")
    assert get_file(link)["status"] == 200
    concierge_sql("UPDATE uploads SET created_at = created_at - 86400", write=True)
    credenza.succeed("systemctl start concierge-expire-uploads")
    assert get_file(link)["status"] == 404
    assert stored_files() == []
