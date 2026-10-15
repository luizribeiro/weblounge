{ lib, ... }:

{
  networking.firewall.enable = true;

  services.openssh = {
    enable = true;
    settings = {
      PasswordAuthentication = false;
      KbdInteractiveAuthentication = false;
      PermitRootLogin = "no";
      # Admins use the box itself; nothing tunnels through it.
      AllowTcpForwarding = false;
      AllowAgentForwarding = false;
      AllowStreamLocalForwarding = false;
      MaxAuthTries = 3;
      LoginGraceTime = 30;
    };
  };

  # Admins authenticate with SSH keys only; there are no user passwords to type.
  security.sudo.wheelNeedsPassword = false;

  nix = {
    settings = {
      experimental-features = [ "nix-command" "flakes" ];
      # The daemon runs as root; services have no business talking to it.
      allowed-users = [ "@wheel" ];
      trusted-users = [ "@wheel" ];
      auto-optimise-store = true;
    };
    gc = {
      automatic = true;
      dates = "weekly";
      options = "--delete-older-than 30d";
    };
  };

  # Less kernel for whoever escapes a service; protectKernelImage also
  # disables kexec.
  boot.kernel.sysctl = {
    "kernel.io_uring_disabled" = 2;
    "kernel.kptr_restrict" = 2;
    "dev.tty.ldisc_autoload" = 0;
    "kernel.yama.ptrace_scope" = 2;
    # One route out leaves no use for redirects, which let anyone on the
    # link reroute traffic.
    # The globs reach interfaces that came up before these applied, and
    # default the ones that come later.
    "net.ipv4.conf.*.accept_redirects" = 0;
    "net.ipv6.conf.*.accept_redirects" = 0;
    "net.ipv4.conf.*.send_redirects" = 0;
  };
  security.protectKernelImage = true;

  # Setuid root binaries nobody here runs are only ways up for an attacker.
  security.wrappers = lib.genAttrs
    [ "fusermount" "fusermount3" "mount" "umount" "newgidmap" "newuidmap" "newgrp" "sg" "su" ]
    (_: { enable = lib.mkForce false; });

  time.timeZone = "UTC";
  i18n.defaultLocale = "en_US.UTF-8";
  boot.tmp.cleanOnBoot = true;
  zramSwap.enable = true;
}
