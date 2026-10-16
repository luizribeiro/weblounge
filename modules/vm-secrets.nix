{ lib, ... }:

# Machines other than production lack its SSH host key, so they decrypt
# stand-ins with a key committed for them. That key is public anyway, so
# passing it through the store is fine; a real key must never be.
let
  keyFile = "sops/vm-age.key";
in
{
  environment.etc.${keyFile} = {
    source = ../secrets/vm-age.key;
    mode = "0400";
  };
  system.activationScripts.setupSecrets.deps = [ "etc" ];

  sops = {
    defaultSopsFile = lib.mkForce ../secrets/vm.yaml;
    age.keyFile = "/etc/${keyFile}";
    age.sshKeyPaths = lib.mkForce [ ];
    gnupg.sshKeyPaths = lib.mkForce [ ];
  };
}
