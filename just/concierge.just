set working-directory := '..'
set positional-arguments

dev_dir := justfile_directory() / "target/dev"
ergo_host := "127.0.0.1"
ergo_port := "8089"
ergo_api := ergo_host + ":" + ergo_port

export CONCIERGE_LISTEN := "127.0.0.1:8080"
export CONCIERGE_PUBLIC_URL := "http://localhost:8080"
export CONCIERGE_ERGO_API := "http://" + ergo_api
export CONCIERGE_ASSETS := dev_dir / "assets"
export STATE_DIRECTORY := dev_dir / "state"
export CREDENTIALS_DIRECTORY := dev_dir / "credentials"

# Run concierge and a throwaway Ergo natively, restarting concierge and reloading the page on every save
dev:
  #!/usr/bin/env bash
  set -euo pipefail
  mkdir -p "$STATE_DIRECTORY" "$CREDENTIALS_DIRECTORY"
  for secret in session-key ergo-api-token; do
    [ -e "$CREDENTIALS_DIRECTORY/$secret" ] || od -An -N32 -tx1 /dev/urandom | tr -d ' \n' > "$CREDENTIALS_DIRECTORY/$secret"
  done
  trap 'kill $(jobs -p) 2>/dev/null' EXIT
  cargo build --quiet --package ergo-api --features local --bin local-ergo
  target/debug/local-ergo "{{dev_dir}}/ergo" {{ergo_api}} "$CREDENTIALS_DIRECTORY/ergo-api-token" \
    > "{{dev_dir}}/ergo.log" 2>&1 &
  ergo=$!
  until (exec 3<>/dev/tcp/{{ergo_host}}/{{ergo_port}}) 2>/dev/null; do
    kill -0 $ergo 2>/dev/null || { cat "{{dev_dir}}/ergo.log"; exit 1; }
    sleep 0.2
  done
  nix run .#web.watchMembers -- web "$CONCIERGE_ASSETS" &
  until [ -e "$CONCIERGE_ASSETS/manifest.json" ]; do sleep 0.2; done
  bacon --headless serve

# Run an admin command against `just concierge dev`, e.g. just concierge dev-admin invite create --root
dev-admin *args:
  @cargo run --quiet --package concierge -- "$@"

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
