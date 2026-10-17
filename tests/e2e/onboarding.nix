{ vmTest, ... }:

vmTest {
  browsers.invite-browser = ./clients/invite-browser.py;
  browsers.verify-browser = ./clients/verify-browser.py;
}
