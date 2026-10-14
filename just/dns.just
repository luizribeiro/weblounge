set working-directory := '..'

token := env("XDG_CONFIG_HOME", env("HOME") / ".config") / "weblounge/desec-token"

# Show how deSEC's zone differs from the one the hosts generate
preview: (dnscontrol "preview")

# Make deSEC's zone match the one the hosts generate
push: (dnscontrol "push")

[private]
dnscontrol command:
  #!/usr/bin/env bash
  set -euo pipefail
  DESEC_TOKEN="$(cat '{{token}}')"
  export DESEC_TOKEN
  config="$(nix build --no-link --print-out-paths .#dnsconfig)"
  dnscontrol {{command}} \
    --config "$config/dnsconfig.js" --creds "$config/creds.json"
