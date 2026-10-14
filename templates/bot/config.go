package main

import (
	"crypto/tls"
	"crypto/x509"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"
)

const defaultServer = "irc.weblounge.org:6697"

type config struct {
	server   string
	nick     string
	password string
	channels []string
	tls      *tls.Config
}

func loadConfig() (config, error) {
	cfg := config{
		server:   os.Getenv("IRC_SERVER"),
		nick:     os.Getenv("IRC_NICK"),
		channels: strings.FieldsFunc(os.Getenv("IRC_CHANNELS"), isListSeparator),
	}
	if cfg.server == "" {
		cfg.server = defaultServer
	}
	if cfg.nick == "" {
		return config{}, errors.New("IRC_NICK is not set: it is the bot's account name and nick")
	}

	var err error
	if cfg.password, err = readPassword(); err != nil {
		return config{}, err
	}
	if cfg.tls, err = tlsConfig(os.Getenv("IRC_CA_FILE")); err != nil {
		return config{}, err
	}
	return cfg, nil
}

func isListSeparator(r rune) bool {
	return r == ',' || r == ' '
}

// readPassword never takes the password from the environment itself, which
// child processes inherit and /proc shows to the bot's user.
func readPassword() (string, error) {
	file := os.Getenv("IRC_PASSWORD_FILE")
	if dir := os.Getenv("CREDENTIALS_DIRECTORY"); dir != "" {
		file = filepath.Join(dir, "password")
	}
	if file == "" {
		return "", errors.New("no password: set IRC_PASSWORD_FILE to a file holding it, " +
			"or run under systemd with LoadCredential=password:<file>")
	}
	data, err := os.ReadFile(file)
	if err != nil {
		return "", fmt.Errorf("reading the password from %s: %w", file, err)
	}
	password := strings.TrimRight(string(data), "\r\n")
	if password == "" {
		return "", fmt.Errorf("the password file %s is empty", file)
	}
	return password, nil
}

// tlsConfig trusts the system's CAs, plus the one in caFile when given,
// such as a test server's self-signed CA.
func tlsConfig(caFile string) (*tls.Config, error) {
	if caFile == "" {
		return &tls.Config{}, nil
	}
	roots, err := x509.SystemCertPool()
	if err != nil {
		return nil, fmt.Errorf("loading the system's CAs: %w", err)
	}
	pem, err := os.ReadFile(caFile)
	if err != nil {
		return nil, fmt.Errorf("reading IRC_CA_FILE: %w", err)
	}
	if !roots.AppendCertsFromPEM(pem) {
		return nil, fmt.Errorf("IRC_CA_FILE %s holds no PEM certificates", caFile)
	}
	return &tls.Config{RootCAs: roots}, nil
}
