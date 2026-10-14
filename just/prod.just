set working-directory := '..'
set positional-arguments

toplevel := ".config.system.build.toplevel"

build host="credenza":
  nix build --no-link -L .#nixosConfigurations.{{host}}{{toplevel}}

# Deploy the current tree to a production host over SSH, once DNS has every name it serves
push host="credenza":
  just dns check
  scripts/wait-for-dns .#nixosConfigurations.{{host}}.config
  scripts/push .#nixosConfigurations.{{host}}{{toplevel}} \
    "$(nix eval --raw .#nixosConfigurations.{{host}}.config.networking.fqdn)"

# First install onto a fresh server, WIPING ITS DISK, e.g. just prod install root@1.2.3.4
install target host="credenza":
  nixos-anywhere --flake .#{{host}} {{target}}

# Run a concierge admin command on credenza, e.g. just prod concierge status
concierge *args:
  #!/usr/bin/env bash
  ssh "$(nix eval --raw .#nixosConfigurations.credenza.config.networking.fqdn)" concierge-admin "$(printf '%q ' "$@")"
