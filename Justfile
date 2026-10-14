mod vm 'just/vm.just'
mod prod 'just/prod.just'
mod web 'just/web.just'
mod concierge 'just/concierge.just'
mod dns 'just/dns.just'

default:
  @just --list --list-submodules

# Everything CI runs: builds, VM tests, lint
check *args:
  nix flake check -L {{args}}

lint:
  pre-commit run --all-files

format:
  nix fmt
