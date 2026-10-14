# CLAUDE.md

NixOS configuration for the weblounge.org IRC network. See README.md for the
workflow and layout.

## Rules

- Never run `just prod push`, `just prod install` or `just dns push`
  without explicit user approval. `just prod build`, `just dns preview`,
  `just check` and everything under `just vm` are fine to run on your own.
- `git add` new files before building: flakes only see tracked files.
- Every host in `hosts/` gets a `toplevel-<host>` flake check and a
  `<host>-vm` package automatically; don't wire them up by hand.
- Behaviour changes to a host should come with a subtest in `tests/`, in
  the file for that area (`host`, `landing`, `ergo`, `concierge`,
  `security`). Every `tests/*.nix` becomes a flake check automatically;
  VM tests are `{ vmTest, ... }: vmTest ''…''` and share `tests/helpers.py`.
- Web pages must work on phones and desktops (fluid layouts, 44px touch
  targets, 16px inputs); check both widths with a headless browser before
  committing UI changes.
- Validate user input on both the frontend and the backend, from one
  definition the server owns.
- Keep VM-only settings under `virtualisation.vmVariant` in
  `modules/vm.nix` so production configs never see them.
- Commit subjects: imperative, at most 72 characters (aim for 50–70);
  details and rationale go in the body.
- Name new servers after furniture (sideboard, armoire, ottoman, chaise…).

## Gotchas

- `ssh-ng://localhost` is treated as the local store by `nix copy`; the VM
  recipes use `127.0.0.1` for that reason.
- `sshd -T` prints keywords in CamelCase; lowercase before matching.
