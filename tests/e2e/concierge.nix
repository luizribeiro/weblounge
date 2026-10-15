{ vmTest, ... }:

vmTest {
  browsers.account-browser = ./clients/account-browser.py;
  browsers.invites-browser = ./clients/invites-browser.py;
}
