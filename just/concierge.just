set working-directory := '..'

dev_dir := "target/dev"

# Run concierge natively on localhost, restarting it and reloading the page on every save
dev port="8080":
  #!/usr/bin/env bash
  set -euo pipefail
  dev="$PWD/{{dev_dir}}"
  mkdir -p "$dev/state" "$dev/credentials"
  for secret in session-key ergo-api-token; do
    [ -e "$dev/credentials/$secret" ] || od -An -N32 -tx1 /dev/urandom | tr -d ' \n' > "$dev/credentials/$secret"
  done
  nix run .#web.watchMembers -- web "$dev/assets" &
  trap 'kill $!' EXIT
  until [ -e "$dev/assets/manifest.json" ]; do sleep 0.2; done
  CONCIERGE_LISTEN=127.0.0.1:{{port}} \
  CONCIERGE_PUBLIC_URL=http://localhost:{{port}} \
  CONCIERGE_ASSETS="$dev/assets" \
  STATE_DIRECTORY="$dev/state" \
  CREDENTIALS_DIRECTORY="$dev/credentials" \
    bacon --headless serve

# Regenerate .sqlx, the query metadata offline builds check against, after changing a query or migration
prepare:
  #!/usr/bin/env bash
  set -euo pipefail
  scratch="$(mktemp -d)"
  trap 'rm -rf "$scratch"' EXIT
  export DATABASE_URL="sqlite://$scratch/prepare.db"
  sqlx database create
  sqlx migrate run --source crates/concierge/migrations
  cd crates/concierge
  cargo sqlx prepare -- --all-targets
