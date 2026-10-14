{ vmTest, ... }:

vmTest ''
  luiz = {"account": "luiz", "password": "plain-old-vanilla"}
  alice = {"account": "alice", "password": "correct-horse"}

  with subtest("bender waits for its owner to be a member"):
      credenza.wait_for_unit("concierge.service")
      credenza.wait_until_fails("systemctl is-active bender-account.service", timeout=60)
      credenza.fail("systemctl is-active weblounge-bot-bender.service")

  with subtest("once its owner is, bender registers itself as their bot"):
      register(**luiz)
      concierge_admin("member", "adopt", "luiz")
      credenza.succeed("systemctl start weblounge-bot-bender.service")
      bots = concierge_admin("bot", "list", "luiz")
      assert re.match(r"bender luiz \d{4}-\d\d-\d\d$", bots), bots

  with subtest("bender joins #bots marked as a bot, and answers !ping"):
      register(**alice)
      client.wait_until_succeeds(irc_command("JOIN #bots", "NAMES #bots", **alice) + " | grep ' 353 alice = #bots :.*bender' >/dev/null", timeout=60)
      session = irc("PRIVMSG #bots :!ping", "WHOIS bender", **alice)
      assert re.search(r"^:bender!\S+ PRIVMSG #bots :alice: pong", session, re.M), session
      assert " 335 alice bender " in session, session

  with subtest("a restart puts back bender's password, whatever it was changed to"):
      concierge_admin("bot", "reset-password", "bender")
      credenza.succeed("systemctl restart bender-account.service weblounge-bot-bender.service")
      client.wait_until_succeeds(irc_command("PRIVMSG bender :!ping", **alice) + " | grep '^:bender!\\S* PRIVMSG alice :pong' >/dev/null", timeout=60)
''
