{ vmTest, ... }:

vmTest ''
  import re
  from html.parser import HTMLParser

  sites = ["weblounge.org", "members.weblounge.org", "irc.weblounge.org"]
  directives_pages_need = {
      "weblounge.org": {"script-src", "style-src", "img-src"},
      "members.weblounge.org": {"style-src", "img-src"},
  }

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
''
