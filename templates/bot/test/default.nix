# Runs the bot through module.nix against an Ergo that requires SASL, and
# pings it in a channel and in a private message.
#
# ergo turns the test's Ergo settings into the server's NixOS module. The
# default runs nixpkgs' Ergo with them; weblounge's flake passes one that
# lays them over weblounge.org's own configuration.
{ pkgs
, ergo ? settings: { services.ergochat = { enable = true; inherit settings; }; }
}:

let
  apiToken = "test-api-token";
  bot = { nick = "bender"; password = "bite-my-shiny-metal"; };
  asker = { account = "alice"; password = "correct-horse"; };

  certificate = pkgs.runCommand "ircd-certificate" { nativeBuildInputs = [ pkgs.openssl ]; } ''
    mkdir $out
    openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:prime256v1 -nodes -days 36500 \
      -subj /CN=ircd -addext subjectAltName=DNS:ircd \
      -keyout $out/key.pem -out $out/cert.pem
  '';

  ircPing = pkgs.writers.writePython3Bin "irc-ping" { } ./irc-ping.py;
in
pkgs.testers.runNixOSTest {
  name = "bot";

  nodes.ircd = {
    imports = [
      (ergo {
        server = {
          name = "irc.test";
          listeners.":6697".tls = {
            cert = "${certificate}/cert.pem";
            key = "${certificate}/key.pem";
          };
        };
        accounts = {
          registration.enabled = false;
          require-sasl = {
            enabled = true;
            exempted = [ ];
          };
        };
        api = {
          enabled = true;
          listener = "127.0.0.1:8089";
          bearer-tokens = [ apiToken ];
        };
      })
    ];
    environment.systemPackages = [ pkgs.curl ircPing ];
    networking.firewall.allowedTCPPorts = [ 6697 ];
  };

  nodes.bot = { lib, ... }: {
    imports = [ ../module.nix ];
    services.weblounge-bot.${bot.nick} = {
      enable = true;
      server = "ircd:6697";
      channels = [ "#bots" ];
      passwordFile = "/run/keys/bot-password";
      caFile = "${certificate}/cert.pem";
    };
    # Started by the test once its account exists.
    systemd.services."weblounge-bot-${bot.nick}".wantedBy = lib.mkForce [ ];
  };

  testScript = ''
    import json
    import re
    import shlex

    def register(account, password):
        request = json.dumps({"accountName": account, "passphrase": password})
        response = ircd.succeed(shlex.join([
            "curl", "-sf", "-H", "Authorization: Bearer ${apiToken}", "-d", request,
            "http://127.0.0.1:8089/v1/saregister",
        ]))
        assert json.loads(response)["success"], response

    def ping(target):
        return ircd.wait_until_succeeds(shlex.join([
            "irc-ping", "--ca", "${certificate}/cert.pem", "ircd:6697",
            "${asker.account}", "${asker.password}", "${bot.nick}", target,
        ]), timeout=60)

    def answer(session, target):
        """The bot's answer, checked to be sent to target and threaded under the ping."""
        echo, reply = [line for line in session.splitlines() if " PRIVMSG " in line][-2:]
        ping_id = re.search(r"\bmsgid=([^;\s]+)", echo)
        reply = re.fullmatch(r"@(\S+) :${bot.nick}!\S+ PRIVMSG (\S+) :(.*)", reply)
        assert ping_id and reply, session
        tags = dict(tag.partition("=")[::2] for tag in reply[1].split(";"))
        assert tags.get("+draft/reply") == ping_id[1], session
        assert reply[2] == target, session
        return tags, reply[3]

    start_all()
    ircd.wait_for_unit("ergochat.service")
    ircd.wait_for_open_port(6697)
    ircd.wait_for_open_port(8089)
    register("${bot.nick}", "${bot.password}")
    register("${asker.account}", "${asker.password}")

    with subtest("the bot logs in with its password, handed over as a credential"):
        bot.succeed("install -D -m 600 /dev/null /run/keys/bot-password")
        bot.succeed("echo ${bot.password} > /run/keys/bot-password")
        bot.succeed("systemctl start weblounge-bot-${bot.nick}")
        bot.wait_until_succeeds("journalctl -u weblounge-bot-${bot.nick} | grep 'connected to ircd:6697 as ${bot.nick}' >/dev/null")

    with subtest("in a channel, the bot answers the asker by name"):
        session = ping("#bots")
        tags, text = answer(session, "#bots")
        assert text == "${asker.account}: pong", session

    with subtest("the bot marks itself as a bot, so Ergo tags its messages"):
        assert "bot" in tags, session

    with subtest("in a private message, the bot answers privately"):
        session = ping("${bot.nick}")
        _, text = answer(session, "${asker.account}")
        assert text == "pong", session

    with subtest("the bot comes back after the server restarts, and answers again"):
        ircd.succeed("systemctl restart ergochat")
        ircd.wait_for_open_port(6697)
        bot.wait_until_succeeds("journalctl -u weblounge-bot-${bot.nick} | grep -c 'connected to ircd:6697' | grep -qx 2", timeout=90)
        tags, text = answer(ping("#bots"), "#bots")
        assert "bot" in tags and text == "${asker.account}: pong", (tags, text)

    with subtest("the bot runs as an unprivileged user and keeps its password out of the journal"):
        uid = bot.succeed("stat -c %u /proc/$(systemctl show -P MainPID weblounge-bot-${bot.nick})").strip()
        assert uid != "0", uid
        bot.fail("journalctl | grep -qF ${bot.password}")
  '';
}
