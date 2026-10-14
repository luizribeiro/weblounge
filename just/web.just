set working-directory := '..'

# Serve web/ on localhost, rebuilding and reloading the page on every save
dev port="8000":
  nix run .#web.dev -- web {{port}}
