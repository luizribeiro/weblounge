{ pkgs, vmTest, ... }:

vmTest ''
  import re
  from html.parser import HTMLParser

  mallory = {"account": "mallory", "password": "an-insider-gone-bad"}
  tls_handshake = "openssl s_client -cipher DEFAULT:@SECLEVEL=0 </dev/null -connect"
  sites = ["weblounge.org", "members.weblounge.org", "irc.weblounge.org"]
  directives_pages_need = {
      "weblounge.org": {"script-src", "style-src", "img-src"},
      "members.weblounge.org": {"style-src", "img-src"},
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

      directives = {"script": "script-src", "img": "img-src", "stylesheet": "style-src", "icon": "img-src"}

      def __init__(self, page):
          super().__init__()
          self.inline = []
          self.fetches = []
          self.feed(page)

      def handle_starttag(self, tag, attrs):
          attrs = dict(attrs)
          if tag == "style" or (tag == "script" and "src" not in attrs) or any(
              name == "style" or name.startswith("on") for name in attrs
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
      credenza.succeed("systemd-run --unit=stray-listener -- ${pkgs.python3}/bin/python3 -m http.server 9000")
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

  with subtest("ergo's API and concierge listen on credenza's loopback only"):
      assert_loopback_only(8089, "/v1/status")
      assert_loopback_only(8080, "/healthz")

  with subtest("ergo's API refuses any token but the generated one"):
      for token in ["not-the-token", "example"]:
          _, status = ergo_api("status", token=token)
          assert status == "401", (token, status)

  with subtest("the API token and session key stay out of the Nix store, readable by root alone"):
      secrets = "/var/lib/weblounge-secrets"
      modes = credenza.succeed(f"stat -c '%a %U' {secrets} {secrets}/ergo-api-token {secrets}/session-key")
      assert modes == "700 root\n600 root\n600 root\n", modes
      for secret, config in [
          ("ergo-api-token", "systemctl cat ergochat; cat /etc/ergo.yaml"),
          ("session-key", "systemctl cat concierge; cat $(readlink -f $(command -v concierge-admin))"),
      ]:
          credenza.fail(f"{{ {config}; }} | grep -qF $(cat {secrets}/{secret})")

  with subtest("default.yaml's example oper and WebIRC gateway, with published credentials, are left out"):
      credenza.fail("grep -qE '^opers:|webirc' /etc/ergo.yaml")

  with subtest("a client that doesn't log in with SASL is turned away, even from loopback"):
      for reply in [irc(), irc(machine=credenza, host="localhost")]:
          assert " 001 " not in reply and re.search(r"^ERROR .*SASL", reply, re.M), reply

  with subtest("NickServ won't register new accounts, even for a member"):
      register(**mallory)
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

  with subtest("the landing page and concierge's load all they need under their CSP, and nothing inline"):
      for name, directives in directives_pages_need.items():
          policy = headers_of(name, "/")["content-security-policy"]
          needs = PageNeeds(https(name, "/", "-f"))
          assert needs.inline == [], (name, needs.inline)
          assert {directive for directive, _ in needs.fetches} == directives, (name, needs.fetches)
          for directive, url in needs.fetches:
              assert re.match(r"/[^/]", url) and csp_sources(policy, directive) == ["'self'"], (name, directive, url, policy)

  with subtest("a POST from another origin, or from nowhere, is refused"):
      for headers in [["-H", "Origin: https://evil.example"], ["-H", "Referer: https://evil.example/"], []]:
          status = https("members.weblounge.org", "/", "-o", "/dev/null", "-w", "%{http_code}", "--data", "", *headers)
          assert status == "403", (headers, status)
''
