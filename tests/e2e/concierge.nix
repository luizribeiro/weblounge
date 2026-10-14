{ vmTest, ... }:

vmTest {
  browsers.account-browser = ./clients/account-browser.py;
}
