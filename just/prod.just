set working-directory := '..'

toplevel := ".config.system.build.toplevel"

build host="credenza":
  nix build --no-link -L .#nixosConfigurations.{{host}}{{toplevel}}

# Deploy the current tree to a production host over SSH
push host="credenza":
  scripts/push .#nixosConfigurations.{{host}}{{toplevel}} \
    "$(nix eval --raw .#nixosConfigurations.{{host}}.config.networking.fqdn)"

# First install onto a fresh server, WIPING ITS DISK, e.g. just prod install root@1.2.3.4
install target host="credenza":
  nixos-anywhere --flake .#{{host}} {{target}}
