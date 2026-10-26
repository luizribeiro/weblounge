{ vmTest, ... }:

vmTest {
  browsers.webchat-browser = ./clients/webchat-browser.py;
  browsers.webchat-notifications-browser = ./clients/webchat-notifications-browser.py;
  browsers.webchat-reconnect-browser = ./clients/webchat-reconnect-browser.py;
  browsers.webchat-settings-browser = ./clients/webchat-settings-browser.py;
  browsers.webchat-upload-browser = ./clients/webchat-upload-browser.py;
}
