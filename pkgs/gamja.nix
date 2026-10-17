{ gamja }:

gamja.overrideAttrs (old: {
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
  ];

  postInstall = (old.postInstall or "") + ''
    install -Dm644 ${../web/images/favicon.svg} $out/favicon.svg
    substituteInPlace $out/index.html --replace-fail \
      '<link rel="manifest"' \
      '<link rel="icon" type="image/svg+xml" href="favicon.svg"><link rel="manifest"'
  '';
})
