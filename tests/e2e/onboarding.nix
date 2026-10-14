{ vmTest, ... }:

vmTest {
  browsers.invite-browser = ./clients/invite-browser.py;
}
