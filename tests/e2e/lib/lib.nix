# Builds a VM test of credenza's real configuration, probed from a client,
# that runs tests/e2e/<name>.py. Takes { credenza, client, browsers, mail,
# webkit } to add to the nodes; browsers maps command names to Playwright
# scripts, `mail = true;` adds the mail node of ./mail.nix, and
# `webkit = true;` lets those scripts run WebKit.
{ pkgs, weblounge, name, script }:

test:
let
  inherit (pkgs) lib;
  mail = lib.optionalAttrs (test.mail or false) (import ./mail.nix { inherit pkgs; });
  # Programs a test runs on credenza that production doesn't have.
  tools = {
    python3 = "${pkgs.python3}/bin/python3";
    sqlite3 = "${pkgs.sqlite}/bin/sqlite3";
    strace = "${pkgs.strace}/bin/strace";
  };
  # Under its name in the repository, so tracebacks point at it.
  runFile = label: file: ''exec(compile(open("${file}").read(), "${label}", "exec"))'';
  # ruff lints it with the rest of the repository's Python.
  ircClient = pkgs.writers.writePython3Bin "irc-client" { doCheck = false; } ../clients/irc-client.py;
  browsers = test.browsers or { };
  python = pkgs.python3.withPackages (ps: [ ps.playwright ]);
  webkit = test.webkit or false;
  # The headless shell, and full Chromium for the notification permissions
  # the shell always denies; WebKit, Safari's engine, for tests that ask.
  playwrightBrowsers = pkgs.playwright-driver.browsers.override {
    withFirefox = false;
    withWebkit = webkit;
    withFfmpeg = false;
  };
  browserModule = pkgs.writeTextDir "browser.py" (builtins.readFile ../clients/browser.py);
  browserCommand = command: script: pkgs.writeShellScriptBin command ''
    PLAYWRIGHT_BROWSERS_PATH=${playwrightBrowsers} PYTHONPATH=${browserModule} \
      exec ${python}/bin/python3 ${script} "$@"
  '';
in
pkgs.testers.runNixOSTest {
  inherit name;

  # credenza sets nixpkgs.hostPlatform, which a read-only pkgs forbids.
  node.pkgsReadOnly = false;

  # The ty pre-commit hook checks the test files, with driver.pyi for the
  # names the driver provides; the driver's own ty can't import it.
  skipTypeCheck = true;

  nodes = {
    credenza = {
      imports = weblounge.hostModules "credenza" ++ [
        ../../../modules/offline-acme.nix
        ../../../modules/vm-secrets.nix
        (mail.credenza or { })
        (test.credenza or { })
        # add_member signs everyone up through an invite, so the greeter
        # would welcome every test's members; welcome.nix turns it on.
        { weblounge.greeter.enable = lib.mkDefault false; }
      ];
      environment.systemPackages = [ pkgs.curl ircClient ];
      # The static-address subtest needs a NIC carrying the VPS's real MAC.
      virtualisation.qemu.options = [
        "-netdev user,id=wan,net=107.173.231.128/25,host=107.173.231.129"
        "-device virtio-net-pci,netdev=wan,mac=00:16:3c:1a:01:be"
      ];
    };
    client = { lib, nodes, ... }: {
      imports = [ (test.client or { }) ];
      # WebKit aborts without an EGL display, and can't be told where a name
      # points, so it looks credenza's up.
      hardware.graphics.enable = webkit;
      networking.hosts = lib.mkIf webkit {
        ${nodes.credenza.networking.primaryIPAddress} = lib.attrNames nodes.credenza.services.nginx.virtualHosts;
      };
      environment.systemPackages = [ pkgs.curl pkgs.netcat pkgs.nmap pkgs.openssl ircClient ]
        ++ lib.mapAttrsToList browserCommand browsers;
      environment.etc.upload-fixtures.source = ../../../crates/scrubber/fixtures;
      virtualisation.memorySize = lib.mkIf (browsers != { }) 2048;
    };
  } // lib.optionalAttrs (mail ? node) { mail = mail.node; };

  testScript = lib.concatLines [
    "tools = ${builtins.toJSON tools}"
    (runFile "tests/e2e/lib/helpers.py" ./helpers.py)
    (runFile "tests/e2e/${name}.py" script)
    (runFile "tests/e2e/lib/epilogue.py" ./epilogue.py)
  ];
}
