//! The greeter: concierge's house account on IRC, which welcomes newcomers
//! in #lounge and tells their inviters. It parses what the network sends
//! it, so it holds as little as it can: concierge's database, and the
//! house account's password, never Ergo's API token.

pub mod irc;
