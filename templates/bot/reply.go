package main

import (
	"strings"

	"github.com/ergochat/irc-go/ircmsg"
)

// reply answers a PRIVMSG where it was asked: in a channel, addressed to
// whoever asked, and in a private message, back to them. When the message
// carries a msgid, the answer is threaded under it with +draft/reply.
func reply(msg ircmsg.Message, text string) (ircmsg.Message, bool) {
	asker := msg.Nick()
	if msg.Command != "PRIVMSG" || len(msg.Params) < 2 || asker == "" {
		return ircmsg.Message{}, false
	}

	target := msg.Params[0]
	if strings.HasPrefix(target, "#") {
		text = asker + ": " + text
	} else {
		target = asker
	}

	answer := ircmsg.MakeMessage(nil, "", "PRIVMSG", target, text)
	if present, msgid := msg.GetTag("msgid"); present {
		answer.SetTag("+draft/reply", msgid)
	}
	return answer, true
}
