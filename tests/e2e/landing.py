import re
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from driver import client, credenza, subtest
    from helpers import assert_client_address_absent, error_page

with subtest("nginx redirects to HTTPS and serves the parking page"):
    credenza.wait_for_unit("nginx.service")
    credenza.wait_for_open_port(443)
    redirect = client.succeed("curl -s -o /dev/null -w '%{redirect_url}' http://credenza/")
    assert redirect == "https://credenza/", redirect
    page = client.succeed("curl -sfk https://credenza/")
    assert "<title>weblounge.org</title>" in page, page

with subtest("the stylesheet, script and favicon the page references are served"):
    assets = re.findall(r'(?:href|src)="(/[^"]+)"', page)
    responses = {
        client.succeed(f"curl -sk -o /dev/null -w '%{{http_code}} %{{content_type}}' https://credenza{asset}")
        for asset in assets
    }
    assert responses == {"200 text/css", "200 text/javascript", "200 image/svg+xml"}, (assets, responses)

with subtest("the wordmark the page draws from a shared file is there under the id it uses"):
    [(wordmark, fragment)] = re.findall(r'<use href="(/assets/[^"#]+)#([^"]+)"', page)
    svg = client.succeed(f"curl -sfk https://credenza{wordmark}")
    assert f'id="{fragment}"' in svg, svg

with subtest("the chair the script animates is inlined from its shared file"):
    assert '<svg xmlns="http://www.w3.org/2000/svg" id="chair"' in page, page
    assert page.count("<circle ") == 53 and "<!-- include" not in page, page

with subtest("the page and its assets are cached briefly and revalidated by ETag"):
    for path in ["/", *assets]:
        headers = client.succeed(f"curl -sfkI https://credenza{path}").lower()
        assert "cache-control: max-age=3600" in headers, (path, headers)
        assert "etag:" in headers, (path, headers)

with subtest("the hashed images the stylesheet loads are cached for good, typed and still guarded"):
    [stylesheet] = [asset for asset in assets if asset.endswith(".css")]
    css = client.succeed(f"curl -sfk https://credenza{stylesheet}")
    textures = set(re.findall(r'url\("\./(felt-[^"]+)"\)', css))
    assert {name.rsplit(".", 1)[1] for name in textures} == {"avif", "png"}, css
    for name in textures:
        headers = client.succeed(f"curl -sfkI https://credenza/assets/{name}").lower()
        content_type = "image/avif" if name.endswith(".avif") else "image/png"
        assert f"content-type: {content_type}" in headers, (name, headers)
        assert "cache-control: public, max-age=31536000, immutable" in headers, (name, headers)
        assert "max-age=3600" not in headers and "x-content-type-options: nosniff" in headers, (name, headers)

with subtest("the release build carries no live-reload code"):
    for path in ["/", *assets]:
        client.succeed(f"curl -sfk -o body https://credenza{path} && ! grep -qE 'EventSource|/esbuild' body")

with subtest("unknown paths get the not-found page, in its own stylesheet and still guarded"):
    lost = error_page("weblounge.org", "/no-such-page", 404)
    assert "<h1>There's nothing here.</h1>" in lost, lost
    assert '<a href="https://weblounge.org/">Back to weblounge.org →</a>' in lost, lost
    assert '<svg xmlns="http://www.w3.org/2000/svg" id="chair"' in lost and "<script" not in lost, lost
    assert error_page("weblounge.org", "/_error/404.html", 404) == lost

with subtest("nginx logs requests without the client's address or query, even when they fail"):
    client.succeed("curl -sk -o /dev/null 'https://credenza/no-such-page?code=a-secret'")
    credenza.succeed("grep -q 'GET /no-such-page HTTP/[0-9.]*\" 404' /var/log/nginx/access.log")
    credenza.fail("grep -q a-secret /var/log/nginx/access.log")
    assert_client_address_absent("{ cat /var/log/nginx/*; journalctl -u nginx; }")
