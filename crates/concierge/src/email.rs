//! The email addresses members may give, and what forms say of them.

pub use common::email::{Email, HTML_PATTERN, MAX_LENGTH};

/// What forms say of an address another member verified.
pub const TAKEN: &str = "Another member uses that address.";
