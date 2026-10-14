# One subtest per promise on concierge's privacy page, under its headings.
# "Not in the logs" is checked after every VM test, in lib.nix.
{ vmTest, pkgs, ... }:

vmTest ''
  def concierge_sql(query):
      return credenza.succeed(shlex.join(
          ["${pkgs.sqlite}/bin/sqlite3", "-readonly", "/var/lib/private/concierge/concierge.db", query]
      ))

  def files_containing(text):
      credenza.succeed("sync")
      return credenza.succeed(f"grep -rlaF {shlex.quote(text)} /var/lib /var/log || true").split()

  grace = {"account": "grace", "password": "willow-harbor-58"}
  graces_browser = Browser("grace")
  email = "grace@example.org"

  credenza.wait_for_unit("concierge.service")
  credenza.wait_for_open_port(8080)
  credenza.wait_for_unit("ergochat.service")
  credenza.wait_for_open_port(8089)
  assert graces_browser.sign_up(concierge_admin("invite", "create", "--root"), **grace) == "200 "

  with subtest("stored: concierge's database has the columns the page was written against"):
      # A new column is something new we store: say so on the page, then add it here.
      columns = concierge_sql(
          "SELECT t.name || '.' || c.name FROM sqlite_schema t, pragma_table_info(t.name) c"
          " WHERE t.type = 'table' AND t.name NOT LIKE '\\_%' ESCAPE '\\' ORDER BY 1"
      ).split()
      assert columns == [
          "bots.account", "bots.created_at", "bots.note", "bots.owner", "bots.removed_at",
          "invites.claimed_at", "invites.code_hash", "invites.created_at", "invites.created_by",
          "invites.expires_at", "invites.makes_admin", "invites.redeemed_at", "invites.redeemed_by",
          "invites.revoked_at",
          "members.account", "members.email", "members.email_verified_at", "members.invited_by",
          "members.is_admin", "members.joined_at", "members.news_opt_in", "members.removed_at",
          "oauth_codes.client_id", "oauth_codes.code_challenge", "oauth_codes.code_hash",
          "oauth_codes.expires_at", "oauth_codes.redirect_uri", "oauth_codes.session_hash",
          "oauth_tokens.client_id", "oauth_tokens.created_at", "oauth_tokens.expires_at",
          "oauth_tokens.session_hash", "oauth_tokens.token_hash",
          "sessions.account", "sessions.created_at", "sessions.expires_at", "sessions.token_hash",
      ], columns

  with subtest("stored: a member's email stays with concierge, out of Ergo's database"):
      files = files_containing(email)
      assert files and all(file.startswith("/var/lib/private/concierge/concierge.db") for file in files), files

  with subtest("not stored: failed sign-ins are counted in memory, and a restart forgets them"):
      for _ in range(5):
          assert graces_browser.sign_in("grace", "a-wrong-guess") == "401 "
      assert graces_browser.sign_in(**grace) == "429 "
      credenza.succeed("systemctl restart concierge")
      credenza.wait_for_open_port(8080)
      assert graces_browser.sign_in(**grace) == f"303 https://{members}/"

  with subtest("email: news emails are off until a member turns them on, and they can turn them off"):
      assert concierge_sql("SELECT news_opt_in FROM members WHERE account = 'grace'") == "0\n"
      assert graces_browser.post("/settings", f"email={email}", "news=on").startswith("303 ")
      assert concierge_sql("SELECT news_opt_in FROM members WHERE account = 'grace'") == "1\n"
      assert graces_browser.post("/settings", f"email={email}").startswith("303 ")
      assert concierge_sql("SELECT news_opt_in FROM members WHERE account = 'grace'") == "0\n"

  with subtest("leaving: an admin deletes the account, its email leaves the disk, and IRC stops taking it"):
      assert " 903 " in irc(**grace)
      concierge_admin("member", "delete", "grace")
      assert files_containing(email) == [], files_containing(email)
      client.fail(irc_command(**grace))
      assert graces_browser.get("/", *outcome) == f"303 https://{members}/signin"
''
