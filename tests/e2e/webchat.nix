{ vmTest, ... }:

vmTest {
  browsers.webchat-browser = ./clients/webchat-browser.py;
  browsers.webchat-reconnect-browser = ./clients/webchat-reconnect-browser.py;
  browsers.webchat-paste-browser = ./clients/webchat-paste-browser.py;
}
