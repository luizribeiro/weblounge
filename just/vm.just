set working-directory := '..'
set positional-arguments

# 127.0.0.1, not localhost: nix copy treats ssh-ng://localhost as the local store.
addr := "127.0.0.1"
ssh_opts := "-p 10022 -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR"

# Boot a host's config locally (ssh :10022, http :10080, https :10443)
start host="credenza":
  mkdir -p .vm
  NIX_DISK_IMAGE=.vm/{{host}}.qcow2 nix run -L .#{{host}}-vm

# Throw away the VM's disk so the next start boots fresh
reset host="credenza":
  rm -f .vm/{{host}}.qcow2

ssh *args:
  ssh {{ssh_opts}} {{addr}} "$@"

# Deploy the current tree into the running VM, the same way prod push does
push host="credenza":
  NIX_SSHOPTS="{{ssh_opts}}" scripts/push .#nixosConfigurations.{{host}}.config.virtualisation.vmVariant.system.build.toplevel {{addr}}
