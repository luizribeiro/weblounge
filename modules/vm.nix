{ lib, ... }:

let
  forwardedPorts = {
    "10022" = 22;
    "10080" = 80;
    "10443" = 443;
  };
in
{
  virtualisation.vmVariant = {
    imports = [ ./offline-acme.nix ];

    virtualisation = {
      cores = 2;
      memorySize = 2048;
      diskSize = 8192;
      graphics = false;
      # Pushes land in the store's overlay; keep it on disk so they survive
      # memory pressure and reboots of the same disk image.
      writableStoreUseTmpfs = false;
      forwardPorts = lib.mapAttrsToList
        (host: guest: {
          from = "host";
          host.port = lib.toInt host;
          guest.port = guest;
        })
        forwardedPorts;
    };

    # QEMU's user-mode network hands out addresses over DHCP, whatever a
    # host does in production.
    networking.useDHCP = lib.mkVMOverride true;

    # QEMU boots the kernel directly, and the disk GRUB would target in
    # production doesn't exist here.
    boot.loader.grub.enable = lib.mkForce false;

    services.getty.autologinUser = "root";
  };
}
