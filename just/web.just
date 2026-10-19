set working-directory := '..'

# Serve web/ on localhost, rebuilding and reloading the page on every save
dev port="8000":
  nix run .#web.dev -- web {{port}}

# Redraw the felt tiles in web/images from web/felt, then check they wrap
bake-felt:
  nix run .#web.bakeFelt -- web
