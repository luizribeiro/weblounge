{
  imports = [
    ./hardware.nix
    ./disko.nix
    ./nginx.nix
  ];

  system.stateVersion = "26.05";
}
