{ vmTest, ... }:

vmTest ''
  import re

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
''
