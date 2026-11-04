# Concierge needs /v1/ns/passwd (2.18) and /v1/ns/saget and saset (2.19);
# drop the version bump once nixpkgs ships them. nixpkgs builds Ergo
# without SQLite, which keeps channel history across restarts.
{ ergochat, fetchFromGitHub, fetchpatch }:

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
    # With allow-truncation on, a DM or highlight too long for one line
    # got no push at all; ours refuses such lines, so this is a safeguard.
    # We plan to offer it upstream.
    ./ergochat-webpush-truncate.patch
    # Maintenance PINGs and MARKREADs went out at high urgency, which costs
    # phones battery. Upstream's PR #2471; drop it once a release has it.
    (fetchpatch {
      url = "https://github.com/ergochat/ergo/commit/9975d8dc0bff2f51e79b7307b9955da375a945fe.patch";
      hash = "sha256-fLQisMdGqdD5CBlxhlN5sD2iMTWWlOhWl+rUkRU19Ec=";
    })
  ];
})
