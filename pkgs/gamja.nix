{ gamja }:

gamja.overrideAttrs (old: {
  # gamja's OAuth 2.0 client sends neither a state nor a PKCE challenge, and
  # puts the page's fragment in its redirect URI; concierge's authorization
  # server requires the first two and refuses the third.
  patches = (old.patches or [ ]) ++ [ ./gamja-oauth2.patch ];

  postInstall = (old.postInstall or "") + ''
    install -Dm644 ${../web/images/favicon.svg} $out/favicon.svg
    substituteInPlace $out/index.html --replace-fail \
      '<link rel="manifest"' \
      '<link rel="icon" type="image/svg+xml" href="favicon.svg"><link rel="manifest"'
  '';
})
