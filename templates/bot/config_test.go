package main

import (
	"os"
	"path/filepath"
	"slices"
	"strings"
	"testing"
)

func setEnv(t *testing.T, vars map[string]string) {
	t.Helper()
	for _, name := range []string{"IRC_SERVER", "IRC_NICK", "IRC_CHANNELS", "IRC_PASSWORD_FILE", "IRC_CA_FILE", "CREDENTIALS_DIRECTORY"} {
		t.Setenv(name, vars[name])
	}
}

func writeFile(t *testing.T, dir, name, content string) string {
	t.Helper()
	path := filepath.Join(dir, name)
	if err := os.WriteFile(path, []byte(content), 0o600); err != nil {
		t.Fatal(err)
	}
	return path
}

func TestLoadConfigFromEnvironment(t *testing.T) {
	dir := t.TempDir()
	setEnv(t, map[string]string{
		"IRC_NICK":          "bender",
		"IRC_CHANNELS":      "#lounge,#bots #robots",
		"IRC_PASSWORD_FILE": writeFile(t, dir, "secret", "bite-my-shiny\n"),
	})

	cfg, err := loadConfig()
	if err != nil {
		t.Fatal(err)
	}
	if cfg.server != "irc.weblounge.org:6697" || cfg.nick != "bender" || cfg.password != "bite-my-shiny" {
		t.Errorf("got %+v", cfg)
	}
	if want := []string{"#lounge", "#bots", "#robots"}; !slices.Equal(cfg.channels, want) {
		t.Errorf("channels %q, want %q", cfg.channels, want)
	}
	if cfg.tls.RootCAs != nil {
		t.Error("trusts more than the system's CAs without IRC_CA_FILE")
	}
}

func TestLoadConfigPrefersSystemdCredentials(t *testing.T) {
	dir := t.TempDir()
	setEnv(t, map[string]string{
		"IRC_SERVER":            "ircd:6697",
		"IRC_NICK":              "bender",
		"IRC_PASSWORD_FILE":     writeFile(t, dir, "secret", "from-the-file"),
		"CREDENTIALS_DIRECTORY": dir,
	})
	writeFile(t, dir, "password", "from-systemd")

	cfg, err := loadConfig()
	if err != nil {
		t.Fatal(err)
	}
	if cfg.server != "ircd:6697" || cfg.password != "from-systemd" || len(cfg.channels) != 0 {
		t.Errorf("got %+v", cfg)
	}
}

func TestLoadConfigTrustsAnExtraCA(t *testing.T) {
	dir := t.TempDir()
	setEnv(t, map[string]string{
		"IRC_NICK":          "bender",
		"IRC_PASSWORD_FILE": writeFile(t, dir, "secret", "hunter2"),
		"IRC_CA_FILE":       writeFile(t, dir, "ca.pem", testCA),
	})

	cfg, err := loadConfig()
	if err != nil {
		t.Fatal(err)
	}
	if cfg.tls.RootCAs == nil {
		t.Error("IRC_CA_FILE was not added to the trusted CAs")
	}
}

func TestLoadConfigErrors(t *testing.T) {
	dir := t.TempDir()
	password := writeFile(t, dir, "secret", "hunter2")
	tests := []struct {
		name string
		env  map[string]string
		want string
	}{
		{"no nick", map[string]string{"IRC_PASSWORD_FILE": password}, "IRC_NICK is not set"},
		{"no password", map[string]string{"IRC_NICK": "bender"}, "no password: set IRC_PASSWORD_FILE"},
		{"missing password file", map[string]string{"IRC_NICK": "bender", "IRC_PASSWORD_FILE": filepath.Join(dir, "nope")}, "reading the password"},
		{"empty password file", map[string]string{"IRC_NICK": "bender", "IRC_PASSWORD_FILE": writeFile(t, dir, "empty", "\n")}, "is empty"},
		{"credentials without a password", map[string]string{"IRC_NICK": "bender", "IRC_PASSWORD_FILE": password, "CREDENTIALS_DIRECTORY": t.TempDir()}, "reading the password"},
		{"CA file that isn't PEM", map[string]string{"IRC_NICK": "bender", "IRC_PASSWORD_FILE": password, "IRC_CA_FILE": password}, "holds no PEM certificates"},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			setEnv(t, tt.env)
			if _, err := loadConfig(); err == nil || !strings.Contains(err.Error(), tt.want) {
				t.Errorf("got error %v, want one containing %q", err, tt.want)
			}
		})
	}
}

const testCA = `-----BEGIN CERTIFICATE-----
MIIBgjCCASmgAwIBAgIUIEEpQbK78wT3hwZoIa691tx8CXIwCgYIKoZIzj0EAwIw
FjEUMBIGA1UEAwwLYm90IHRlc3QgQ0EwIBcNMjYxMDE0MTg0MDQzWhgPMjEyNjA5
MjAxODQwNDNaMBYxFDASBgNVBAMMC2JvdCB0ZXN0IENBMFkwEwYHKoZIzj0CAQYI
KoZIzj0DAQcDQgAEejF8erodfW9tTTF+yb7cwxGFt87ERlo3Bcwp7BNs5QttH6zQ
BRovdkMs4mNIgewQUFgW+mmMpc6lo/NoCWfWsaNTMFEwHQYDVR0OBBYEFGPqC5RQ
gk3cjCLpv1FD0cX70zDRMB8GA1UdIwQYMBaAFGPqC5RQgk3cjCLpv1FD0cX70zDR
MA8GA1UdEwEB/wQFMAMBAf8wCgYIKoZIzj0EAwIDRwAwRAIgOYAYaQoe1SkG8KCQ
KG/CnGksylsqPd9YjFDL5hyFItwCIDam2GrlUfpeQL4hQR/U4qNDgyf2gN4mqaP+
/X6Nm4KR
-----END CERTIFICATE-----
`
