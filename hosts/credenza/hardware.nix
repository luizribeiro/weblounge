{ modulesPath, ... }:

{
  imports = [ (modulesPath + "/profiles/qemu-guest.nix") ];

  nixpkgs.hostPlatform = "x86_64-linux";

  boot.initrd.availableKernelModules = [
    "ahci"
    "sd_mod"
    "sr_mod"
    "virtio_pci"
    "virtio_scsi"
    "xhci_pci"
  ];

  # Boots on both BIOS and UEFI VPSes, since we haven't picked a provider yet.
  boot.loader.grub = {
    efiSupport = true;
    efiInstallAsRemovable = true;
  };
}
