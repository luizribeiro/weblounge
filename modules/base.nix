{
  networking.firewall.enable = true;

  services.openssh = {
    enable = true;
    settings = {
      PasswordAuthentication = false;
      KbdInteractiveAuthentication = false;
      PermitRootLogin = "no";
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
  };
  security.protectKernelImage = true;

  time.timeZone = "UTC";
  i18n.defaultLocale = "en_US.UTF-8";
  boot.tmp.cleanOnBoot = true;
  zramSwap.enable = true;
}
