//! The little markup help articles are written in: `code`, **strong** and
//! [a link](/path). Anything else, an unclosed mark included, is text.
//! Templates escape every part, so no markup can make HTML of its own.

#[derive(Debug, PartialEq, Eq)]
pub enum Span<'a> {
    Text(&'a str),
    Code(&'a str),
    Strong(&'a str),
    Link { label: &'a str, href: &'a str },
}

/// `text` split into the spans its marks make.
pub fn spans(text: &str) -> Vec<Span<'_>> {
    let mut spans = Vec::new();
    let mut text_from = 0;
    let mut at = 0;
    while at < text.len() {
        match marked(&text[at..]) {
            Some((span, length)) => {
                if text_from < at {
                    spans.push(Span::Text(&text[text_from..at]));
                }
                spans.push(span);
                at += length;
                text_from = at;
            }
            None => at += text[at..].chars().next().map_or(1, char::len_utf8),
        }
    }
    if text_from < text.len() {
        spans.push(Span::Text(&text[text_from..]));
    }
    spans
}

/// The span a mark at the start of `text` makes, and how long it is.
fn marked(text: &str) -> Option<(Span<'_>, usize)> {
    if let Some(rest) = text.strip_prefix("**") {
        let (inner, _) = rest.split_once("**")?;
        Some((Span::Strong(inner), inner.len() + 4))
    } else if let Some(rest) = text.strip_prefix('`') {
        let (inner, _) = rest.split_once('`')?;
        Some((Span::Code(inner), inner.len() + 2))
    } else if let Some(rest) = text.strip_prefix('[') {
        let (label, rest) = rest
            .split_once("](")
            .filter(|(label, _)| !label.contains(']'))?;
        let (href, _) = rest.split_once(')')?;
        Some((Span::Link { label, href }, label.len() + href.len() + 4))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Span::*;

    #[test]
    fn plain_text_is_one_span() {
        assert_eq!(
            spans("You stay in your channels."),
            [Text("You stay in your channels.")]
        );
        assert_eq!(spans(""), []);
    }

    #[test]
    fn marks_code_strong_and_links_between_text() {
        assert_eq!(
            spans("Run `/join #name` on the [Account](/account#irc-settings) tab, then **Save**."),
            [
                Text("Run "),
                Code("/join #name"),
                Text(" on the "),
                Link {
                    label: "Account",
                    href: "/account#irc-settings"
                },
                Text(" tab, then "),
                Strong("Save"),
                Text("."),
            ]
        );
    }

    #[test]
    fn marks_at_either_end_leave_no_empty_text() {
        assert_eq!(
            spans("`+B` and **IRC while you're away**"),
            [Code("+B"), Text(" and "), Strong("IRC while you're away")]
        );
    }

    #[test]
    fn unclosed_marks_stay_text() {
        for text in [
            "a ` alone",
            "**half strong",
            "[label] (no link)",
            "[label](/no-end",
            "[a] then [b](/c",
        ] {
            assert_eq!(spans(text), [Text(text)], "{text}");
        }
    }

    #[test]
    fn text_keeps_whatever_else_it_holds() {
        assert_eq!(
            spans("Images up to 10 MB, <b> & all · é"),
            [Text("Images up to 10 MB, <b> & all · é")]
        );
    }
}
