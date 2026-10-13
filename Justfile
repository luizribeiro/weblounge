mod vm 'just/vm.just'
mod prod 'just/prod.just'

default:
  @just --list --list-submodules

# Everything CI and the pre-push hook run: builds, VM tests, lint
check *args:
  nix flake check -L {{args}}

lint:
  pre-commit run --all-files

format:
  nix fmt
