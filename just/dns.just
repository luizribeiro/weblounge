set working-directory := '..'

# Show how deSEC's zone differs from the one the hosts generate
preview: (dnscontrol "preview")

# Fail if deSEC's zone differs from the one the hosts generate
check: (dnscontrol "preview --expect-no-changes")

# Make deSEC's zone match the one the hosts generate
push: (dnscontrol "push")

[private]
dnscontrol command:
  #!/usr/bin/env bash
  set -euo pipefail
  config="$(nix build --no-link --print-out-paths .#dnsconfig)"
  sops exec-env secrets/dns.yaml \
    "dnscontrol {{command}} --config $config/dnsconfig.js --creds $config/creds.json"
