package main

import (
	"strings"
	"testing"

	"github.com/ergochat/irc-go/ircmsg"
)

func TestReply(t *testing.T) {
	tests := []struct {
		name, received, want string
	}{
		{
			name:     "in a channel, addressed to the asker and threaded",
			received: "@msgid=Zx9;time=2026-10-14T12:00:00.000Z :alice!~u@cloak.weblounge PRIVMSG #lounge :!ping",
			want:     "@+draft/reply=Zx9 PRIVMSG #lounge :alice: pong",
		},
		{
			name:     "in a private message, back to the asker and threaded",
			received: "@msgid=Ab3 :alice!~u@cloak.weblounge PRIVMSG bender :!ping",
			want:     "@+draft/reply=Ab3 PRIVMSG alice pong",
		},
		{
			name:     "in a channel, without message-tags",
			received: ":alice!~u@cloak.weblounge PRIVMSG #lounge :!ping",
			want:     "PRIVMSG #lounge :alice: pong",
		},
		{
			name:     "in a private message, without message-tags",
			received: ":alice!~u@cloak.weblounge PRIVMSG bender :!ping",
			want:     "PRIVMSG alice pong",
		},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			answer, ok := reply(parse(t, tt.received), "pong")
			if !ok {
				t.Fatal("no reply")
			}
			if got := line(t, answer); got != tt.want {
				t.Errorf("got %q, want %q", got, tt.want)
			}
		})
	}
}

func TestReplyIgnoresWhatItCannotAnswer(t *testing.T) {
	for _, received := range []string{
		":irc.weblounge.org NOTICE bender :*** Looking up your hostname...",
		"PRIVMSG bender :!ping",
		":alice!~u@cloak.weblounge PRIVMSG bender",
	} {
		if answer, ok := reply(parse(t, received), "pong"); ok {
			t.Errorf("%q: answered %q", received, line(t, answer))
		}
	}
}

func parse(t *testing.T, raw string) ircmsg.Message {
	t.Helper()
	msg, err := ircmsg.ParseLine(raw)
	if err != nil {
		t.Fatalf("parsing %q: %v", raw, err)
	}
	return msg
}

func line(t *testing.T, msg ircmsg.Message) string {
	t.Helper()
	raw, err := msg.Line()
	if err != nil {
		t.Fatal(err)
	}
	return strings.TrimSuffix(raw, "\r\n")
}
