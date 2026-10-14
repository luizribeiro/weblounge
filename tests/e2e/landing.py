import re
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from driver import client, credenza, subtest
    from helpers import assert_client_address_absent

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

with subtest("the release build carries no live-reload code"):
    for path in ["/", *assets]:
        client.succeed(f"curl -sfk -o body https://credenza{path} && ! grep -qE 'EventSource|/esbuild' body")

with subtest("nginx logs requests without the client's address, even when they fail"):
    client.succeed("curl -sk -o /dev/null https://credenza/no-such-page")
    credenza.succeed("grep -q 'GET /no-such-page HTTP/[0-9.]*\" 404' /var/log/nginx/access.log")
    assert_client_address_absent("{ cat /var/log/nginx/*; journalctl -u nginx; }")
