# Builds a VM test of credenza's real configuration, probed from a client,
# that runs tests/e2e/<name>.py. Takes { credenza, client, browsers } to
# add to the nodes; browsers maps command names to Playwright scripts.
{ pkgs, weblounge, name, script }:

test:
let
  inherit (pkgs) lib;
  # Programs a test runs on credenza that production doesn't have.
  tools = {
    python3 = "${pkgs.python3}/bin/python3";
    sqlite3 = "${pkgs.sqlite}/bin/sqlite3";
  };
  # Under its name in the repository, so tracebacks point at it.
  runFile = label: file: ''exec(compile(open("${file}").read(), "${label}", "exec"))'';
  # ruff lints it with the rest of the repository's Python.
  ircClient = pkgs.writers.writePython3Bin "irc-client" { doCheck = false; } ../clients/irc-client.py;
  browsers = test.browsers or { };
  python = pkgs.python3.withPackages (ps: [ ps.playwright ]);
  # The headless shell is all a headless test needs.
  playwrightBrowsers = pkgs.playwright-driver.browsers.override {
    withChromium = false;
    withFirefox = false;
    withWebkit = false;
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
      imports = weblounge.hostModules "credenza" ++ [ ../../../modules/offline-acme.nix (test.credenza or { }) ];
      environment.systemPackages = [ pkgs.curl ircClient ];
      # The static-address subtest needs a NIC carrying the VPS's real MAC.
      virtualisation.qemu.options = [
        "-netdev user,id=wan,net=107.173.231.128/25,host=107.173.231.129"
        "-device virtio-net-pci,netdev=wan,mac=00:16:3c:1a:01:be"
      ];
    };
    client = {
      imports = [ (test.client or { }) ];
      environment.systemPackages = [ pkgs.curl pkgs.netcat pkgs.nmap pkgs.openssl ircClient ]
        ++ lib.mapAttrsToList browserCommand browsers;
      virtualisation.memorySize = lib.mkIf (browsers != { }) 2048;
    };
  };

  testScript = lib.concatLines [
    "tools = ${builtins.toJSON tools}"
    (runFile "tests/e2e/lib/helpers.py" ./helpers.py)
    (runFile "tests/e2e/${name}.py" script)
    (runFile "tests/e2e/lib/epilogue.py" ./epilogue.py)
  ];
}
