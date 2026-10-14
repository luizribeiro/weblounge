# Concierge needs /v1/ns/passwd (2.18) and /v1/ns/saget and saset (2.19);
# drop this once nixpkgs ships them.
{ ergochat, fetchFromGitHub }:

ergochat.overrideAttrs (finalAttrs: _: {
  version = "2.19.1";
  src = fetchFromGitHub {
    owner = "ergochat";
    repo = "ergo";
    rev = "v${finalAttrs.version}";
    hash = "sha256-yzGLOpECalSOv1zBpVkyDlHGaHSsQNsAoNa2jgLpsgM=";
  };
})
