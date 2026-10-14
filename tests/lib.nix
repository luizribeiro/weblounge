# Builds a VM test of credenza's real configuration, probed from a client.
{ pkgs, weblounge, name }:

testScript:
let
  ircClient = pkgs.writers.writePython3Bin "irc-client" { } ./irc-client.py;
in
pkgs.testers.runNixOSTest {
  inherit name;

  # credenza sets nixpkgs.hostPlatform, which a read-only pkgs forbids.
  node.pkgsReadOnly = false;

  nodes = {
    credenza = {
      imports = weblounge.hostModules "credenza" ++ [ ../modules/offline-acme.nix ];
      environment.systemPackages = [ pkgs.curl ircClient ];
      # The static-address subtest needs a NIC carrying the VPS's real MAC.
      virtualisation.qemu.options = [
        "-netdev user,id=wan,net=107.173.231.128/25,host=107.173.231.129"
        "-device virtio-net-pci,netdev=wan,mac=00:16:3c:1a:01:be"
      ];
    };
    client.environment.systemPackages = [ pkgs.curl pkgs.netcat pkgs.nmap pkgs.openssl ircClient ];
  };

  testScript = ''
    ${builtins.readFile ./helpers.py}

    ${testScript}

    # sshd's log is the admins' record of who got into the server; it names
    # admins and scanners, never members.
    with subtest("after everything above, only sshd's log names a client address"):
        assert_client_address_absent("journalctl | grep -vE ' sshd(-session|-auth)?\\[[0-9]+\\]: '")
  '';
}
