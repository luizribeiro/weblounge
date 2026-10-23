# shellcheck shell=bash
# Usage: ops-notify LINE...
# Posts LINE to #ops through louro, waiting up to OPS_NOTIFY_PATIENCE
# seconds (120) for louro to be connected and in #ops, so lines from
# early in a boot aren't lost.

line="$*"
if [ -z "$line" ]; then
  echo "usage: ops-notify LINE..." >&2
  exit 2
fi

deadline=$((SECONDS + ${OPS_NOTIFY_PATIENCE:-120}))

in_ops() {
  louro status 2>/dev/null | jq -e '.connected and (.channels | index("#ops")) != null' >/dev/null
}

error="louro isn't in #ops"
while true; do
  if in_ops; then
    status=0
    error=$(louro send -- '#ops' "$line" 2>&1 >/dev/null) || status=$?
    case $status in
      0) exit 0 ;;
      # louro refuses the line itself, which no retry changes.
      2) echo "ops-notify: $error" >&2; exit 2 ;;
    esac
  fi
  if ((SECONDS >= deadline)); then
    echo "ops-notify: dropped \"$line\": $error" >&2
    exit 1
  fi
  sleep 2
done
