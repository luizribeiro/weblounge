// A weblounge.org IRC bot: it answers !ping with pong. Add commands in
// onMessage.
package main

import (
	"context"
	"log"
	"os"
	"os/signal"
	"strings"
	"syscall"
	"time"

	"github.com/ergochat/irc-go/ircevent"
	"github.com/ergochat/irc-go/ircmsg"
)

func main() {
	log.SetFlags(0)
	cfg, err := loadConfig()
	if err != nil {
		log.Fatal(err)
	}

	irc := &ircevent.Connection{
		Server:        cfg.server,
		Nick:          cfg.nick,
		UseTLS:        true,
		TLSConfig:     cfg.tls,
		SASLLogin:     cfg.nick,
		SASLPassword:  cfg.password,
		RequestCaps:   []string{"message-tags", "server-time"},
		ReconnectFreq: 30 * time.Second,
		QuitMessage:   "bye",
		Log:           log.Default(),
	}
	irc.AddConnectCallback(func(ircmsg.Message) {
		irc.Send("MODE", irc.CurrentNick(), "+B")
		for _, channel := range cfg.channels {
			irc.Join(channel)
		}
	})
	irc.AddCallback("PRIVMSG", func(msg ircmsg.Message) {
		onMessage(irc, msg)
	})

	if err := irc.Connect(); err != nil {
		log.Fatalf("connecting to %s: %v", cfg.server, err)
	}
	log.Printf("connected to %s as %s", cfg.server, irc.CurrentNick())

	// Installed only once connected: a Quit during Connect would leave Loop
	// waiting on a connection nothing closes.
	ctx, stop := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
	defer stop()
	go func() {
		<-ctx.Done()
		irc.Quit()
	}()
	irc.Loop()
}

func onMessage(irc *ircevent.Connection, msg ircmsg.Message) {
	if len(msg.Params) < 2 {
		return
	}
	switch strings.TrimSpace(msg.Params[1]) {
	case "!ping":
		if answer, ok := reply(msg, "pong"); ok {
			send(irc, answer)
		}
	}
}

func send(irc *ircevent.Connection, msg ircmsg.Message) {
	if err := irc.SendIRCMessage(msg); err != nil {
		log.Printf("sending %s to %s: %v", msg.Command, msg.Params[0], err)
	}
}
