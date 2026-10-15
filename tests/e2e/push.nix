{ vmTest, pkgs, ... }:

let
  # Systems to push, each credenza with something added. They're in its own
  # closure already, so a push copies little and needs no network.
  variants = { lib, ... }: {
    healthy.environment.etc.pushed.text = "healthy";
    locked-out = {
      environment.etc.pushed.text = "locked-out";
      services.openssh.settings.AllowUsers = lib.mkForce [ "nobody" ];
    };
    failing-on-host = {
      environment.etc.pushed.text = "failing-on-host";
      weblounge.deployChecks."never passes on the host" = { script = "false"; timeout = 1; };
    };
    failing-outside = {
      environment.etc.pushed.text = "failing-outside";
      weblounge.deployChecks."never passes from outside" = { runs = "outside"; script = "false"; timeout = 1; };
    };
  };

  push = pkgs.writeShellApplication {
    name = "push";
    runtimeInputs = [ pkgs.nix pkgs.openssh ];
    text = builtins.readFile ../../scripts/push;
  };
in
vmTest {
  credenza = { lib, ... }: {
    # The test boots the kernel directly; there's no disk GRUB could go on.
    boot.loader.grub.enable = lib.mkForce false;
    specialisation = lib.mapAttrs (_: configuration: { inherit configuration; }) (variants { inherit lib; });
  };

  # The machine pushing, with each variant at /etc/variants/<name>.
  client = { lib, nodes, ... }: {
    nix.settings.experimental-features = [ "nix-command" ];
    # Outside checks reach credenza by its public names, as visitors do.
    networking.hosts.${nodes.credenza.networking.primaryIPAddress} =
      lib.attrNames nodes.credenza.services.nginx.virtualHosts;
    environment.systemPackages = [ push ];
    environment.etc = lib.mapAttrs'
      (name: _: lib.nameValuePair "variants/${name}" {
        source = "${nodes.credenza.system.build.toplevel}/specialisation/${name}";
      })
      (variants { inherit lib; });
    virtualisation.additionalPaths = [ nodes.credenza.system.build.toplevel ];
  };
}
