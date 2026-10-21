//! The Help tab's topics: short answers to what members ask about most.

/// A page Help links to, by the task it helps with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Topic {
    pub title: &'static str,
    pub href: &'static str,
}

/// The client guides, which have pages of their own.
pub const OWN_APP: Topic = Topic {
    title: "Use your own app",
    href: "/clients",
};

/// What the Help index lists, in order.
pub fn index() -> Vec<Topic> {
    vec![OWN_APP]
}
