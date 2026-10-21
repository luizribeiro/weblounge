# Concierge needs /v1/ns/passwd (2.18) and /v1/ns/saget and saset (2.19);
# drop the version bump once nixpkgs ships them. nixpkgs builds Ergo
# without SQLite, which keeps channel history across restarts.
{ ergochat, fetchFromGitHub }:

ergochat.overrideAttrs (finalAttrs: old: {
  version = "2.19.1";
  src = fetchFromGitHub {
    owner = "ergochat";
    repo = "ergo";
    rev = "v${finalAttrs.version}";
    hash = "sha256-yzGLOpECalSOv1zBpVkyDlHGaHSsQNsAoNa2jgLpsgM=";
  };
  tags = [ "sqlite" ];
  patches = (old.patches or [ ]) ++ [
    # Bots get their vhosts from concierge, which only has the API, and
    # Ergo only sets vhosts through an oper's HS SET. Not offered upstream
    # yet.
    ./ergochat-hs-set-api.patch
    # Deleting a bot from the members' site has to disconnect it, which
    # only an oper's NS ERASE does. Not offered upstream yet.
    ./ergochat-ns-erase-api.patch
    # Members manage their accounts through concierge, which Ergo's own
    # NickServ and HostServ commands would bypass. Not offered upstream yet.
    ./ergochat-disable-service-commands.patch
  ];
})
