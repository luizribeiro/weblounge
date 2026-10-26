{ gamja, fetchFromCodeberg, fetchNpmDeps }:

gamja.overrideAttrs (finalAttrs: old: {
  version = "1.0.0-beta.11-unstable-2026-09-13";

  src = fetchFromCodeberg {
    owner = "emersion";
    repo = "gamja";
    rev = "59ed8b13efadc4753efc29afb8e9437bd3df3875";
    hash = "sha256-nLxmKMxcY9gfIrgvFJvm4vDWkESz4wzZKDLNCqu4sIE=";
  };

  npmDeps = fetchNpmDeps {
    inherit (finalAttrs) src;
    name = "${finalAttrs.pname}-${finalAttrs.version}-npm-deps";
    hash = "sha256-BMiDE1rBb4XkWEmGxhkZPF5hDoV4BksfwU094BuBkKI=";
  };

  patches = (old.patches or [ ]) ++ [
    # gamja's OAuth 2.0 client sends neither a state nor a PKCE challenge,
    # and puts the page's fragment in its redirect URI; concierge's
    # authorization server requires the first two and refuses the third.
    # State and PKCE are in upstream's open PR
    # https://codeberg.org/emersion/gamja/pulls/210, the fragment isn't:
    # https://codeberg.org/emersion/gamja/pulls/210#issuecomment-25478230
    ./gamja-oauth2.patch
    # gamja's catch-up after a reconnect asks for everything after the last
    # line it has, so lines that arrive live meanwhile come twice. Offered
    # upstream, implemented differently, in
    # https://codeberg.org/emersion/gamja/pulls/236
    ./gamja-dedup-msgid.patch
    # Members asked to change the text's size and typeface; gamja's settings
    # have neither. Written to be offered upstream.
    ./gamja-font-settings.patch
    # gamja sends the nick as the real name; a fixed one from config.json
    # tells the network a connection is the web client's. Written to be
    # offered upstream.
    ./gamja-realname.patch
  ];

  postInstall = (old.postInstall or "") + ''
    install -Dm644 ${../web/images/favicon.svg} $out/favicon.svg
    substituteInPlace $out/index.html --replace-fail \
      '<link rel=manifest' \
      '<link rel="icon" type="image/svg+xml" href="favicon.svg"><link rel=manifest'
  '';
})
