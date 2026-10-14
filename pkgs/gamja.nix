# gamja's OAuth 2.0 client sends neither a state nor a PKCE challenge, and
# puts the page's fragment in its redirect URI; concierge's authorization
# server requires the first two and refuses the third.
{ gamja }:

gamja.overrideAttrs (old: {
  patches = (old.patches or [ ]) ++ [ ./gamja-oauth2.patch ];
})
