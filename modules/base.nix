{
  networking.domain = "weblounge.org";
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
      trusted-users = [ "@wheel" ];
      auto-optimise-store = true;
    };
    gc = {
      automatic = true;
      dates = "weekly";
      options = "--delete-older-than 30d";
    };
  };

  time.timeZone = "UTC";
  i18n.defaultLocale = "en_US.UTF-8";
  boot.tmp.cleanOnBoot = true;
  zramSwap.enable = true;
}
