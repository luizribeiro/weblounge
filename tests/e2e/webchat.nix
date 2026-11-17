{ vmTest, ... }:

vmTest {
  webkit = true;
  browsers.client-signin-browser = ./clients/client-signin-browser.py;
}
