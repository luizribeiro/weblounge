{ vmTest, ... }:

vmTest {
  browsers.webchat-browser = ./clients/webchat-browser.py;
}
