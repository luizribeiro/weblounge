# Concierge needs /v1/ns/passwd (2.18) and /v1/ns/saget and saset (2.19);
# drop the version bump once nixpkgs ships them. nixpkgs builds Ergo
# without SQLite, which keeps channel history across restarts.
{ ergochat, fetchFromGitHub }:

ergochat.overrideAttrs (finalAttrs: _: {
  version = "2.19.1";
  src = fetchFromGitHub {
    owner = "ergochat";
    repo = "ergo";
    rev = "v${finalAttrs.version}";
    hash = "sha256-yzGLOpECalSOv1zBpVkyDlHGaHSsQNsAoNa2jgLpsgM=";
  };
  tags = [ "sqlite" ];
})
