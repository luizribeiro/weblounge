{ vmTest, ... }:

vmTest {
  mail = true;
  browsers.recovery-browser = ./clients/recovery-browser.py;
}
