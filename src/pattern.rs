//! Pattern matching notation (POSIX §2.13.1): `*`, `?`, bracket expressions
//! and `\` quoting. For the `${x%pattern}` family now; `case` and pathname
//! expansion later.
//!
//! The pattern is text in which a quoted character is written with a `\`
//! before it: the expansion escapes what was quoted in the shell word
//! (`${x#"*"}` is the pattern `\*`).
//!
//! TODO: collating symbols (`[.x.]`), equivalence classes (`[=x=]`), and the
//! rules of pathname expansion (a leading `.`, `/` matched only literally).

/// A parsed pattern.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pattern {
    pieces: Vec<Piece>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Piece {
    /// A character that matches itself.
    Char(char),
    /// `?`: any character.
    Any,
    /// `*`: any string, the empty one too.
    Star,
    /// `[...]`.
    Bracket(Bracket),
}

/// A bracket expression: one character among the items, or not among them
/// with `negated` (`[!...]`, `[^...]`).
#[derive(Debug, Clone, PartialEq, Eq)]
struct Bracket {
    negated: bool,
    items: Vec<BracketItem>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum BracketItem {
    Char(char),
    /// `a-z`, both ends included.
    Range(char, char),
    /// `[:alpha:]`...
    Class(Class),
}

/// The character classes of a bracket expression (XBD §9.3.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Class {
    Alpha,
    Digit,
    Alnum,
    Upper,
    Lower,
    Space,
    Blank,
    Punct,
    Print,
    Graph,
    Cntrl,
    Xdigit,
}

impl Pattern {
    /// Parse `pattern`. A `[` that doesn't start a valid bracket expression
    /// matches itself, like an unquoted one in any shell.
    pub fn new(pattern: &str) -> Self {
        let chars: Vec<char> = pattern.chars().collect();
        let mut pieces = Vec::new();
        let mut i = 0;

        while i < chars.len() {
            let piece = match chars[i] {
                '\\' if i + 1 < chars.len() => {
                    i += 1;
                    Piece::Char(chars[i])
                }
                // `**` is the same as `*`.
                '*' if pieces.last() == Some(&Piece::Star) => {
                    i += 1;
                    continue;
                }
                '*' => Piece::Star,
                '?' => Piece::Any,
                '[' => match Bracket::parse(&chars[i + 1..]) {
                    Some((bracket, len)) => {
                        i += len;
                        Piece::Bracket(bracket)
                    }
                    None => Piece::Char('['),
                },
                c => Piece::Char(c),
            };
            pieces.push(piece);
            i += 1;
        }

        Self { pieces }
    }

    /// The whole of `text` matches the pattern.
    pub fn matches(&self, text: &str) -> bool {
        let text: Vec<char> = text.chars().collect();
        let pieces = &self.pieces;

        let (mut p, mut t) = (0, 0);
        // Where to go on from when what follows the last `*` fails: the piece
        // after it, and the text it would then have taken one more of.
        let mut backtrack = None;

        while t < text.len() {
            match pieces.get(p) {
                Some(Piece::Star) => {
                    p += 1;
                    backtrack = Some((p, t + 1));
                    continue;
                }
                Some(piece) if piece.matches(text[t]) => {
                    p += 1;
                    t += 1;
                    continue;
                }
                _ => {}
            }

            match backtrack {
                Some((after_star, next)) => {
                    p = after_star;
                    t = next;
                    backtrack = Some((after_star, next + 1));
                }
                None => return false,
            }
        }

        pieces[p..].iter().all(|piece| *piece == Piece::Star)
    }

    /// `text` without the smallest (or the `largest`) prefix the pattern
    /// matches, unchanged when none does: `${x#pattern}`, `${x##pattern}`.
    pub fn strip_prefix<'t>(&self, text: &'t str, largest: bool) -> &'t str {
        let mut ends = boundaries(text);
        if largest {
            ends.reverse();
        }

        match ends.into_iter().find(|&end| self.matches(&text[..end])) {
            Some(end) => &text[end..],
            None => text,
        }
    }

    /// `text` without the smallest (or the `largest`) suffix the pattern
    /// matches, unchanged when none does: `${x%pattern}`, `${x%%pattern}`.
    pub fn strip_suffix<'t>(&self, text: &'t str, largest: bool) -> &'t str {
        let mut starts = boundaries(text);
        if !largest {
            starts.reverse();
        }

        match starts
            .into_iter()
            .find(|&start| self.matches(&text[start..]))
        {
            Some(start) => &text[..start],
            None => text,
        }
    }
}

/// Every char boundary of `text`, from 0 to its length.
fn boundaries(text: &str) -> Vec<usize> {
    text.char_indices()
        .map(|(i, _)| i)
        .chain(std::iter::once(text.len()))
        .collect()
}

impl Piece {
    /// One character of the text matches this piece (not a `*`).
    fn matches(&self, c: char) -> bool {
        match self {
            Self::Char(expected) => *expected == c,
            Self::Any => true,
            Self::Star => false,
            Self::Bracket(bracket) => bracket.matches(c),
        }
    }
}

impl Bracket {
    /// The bracket expression after a `[`, and how many characters it takes
    /// with its `]`. `None` when it is not one (no `]`, a bad class).
    fn parse(chars: &[char]) -> Option<(Self, usize)> {
        let mut i = 0;
        let negated = matches!(chars.first(), Some('!' | '^'));
        if negated {
            i += 1;
        }

        let mut items = Vec::new();
        let start = i;
        loop {
            let c = *chars.get(i)?;
            // A `]` first is an ordinary character.
            if c == ']' && i > start {
                return Some((Self { negated, items }, i + 1));
            }

            if c == '[' && chars.get(i + 1) == Some(&':') {
                let rest = &chars[i + 2..];
                let end = rest.windows(2).position(|w| w == [':', ']'])?;
                let name: String = rest[..end].iter().collect();
                items.push(BracketItem::Class(Class::from_name(&name)?));
                i += 2 + end + 2;
                continue;
            }

            let (first, len) = escaped(&chars[i..])?;
            i += len;

            // A range, unless the `-` is last (`[a-]`).
            if chars.get(i) == Some(&'-') && chars.get(i + 1).is_some_and(|&c| c != ']') {
                let (last, len) = escaped(&chars[i + 1..])?;
                i += 1 + len;
                items.push(BracketItem::Range(first, last));
            } else {
                items.push(BracketItem::Char(first));
            }
        }
    }

    fn matches(&self, c: char) -> bool {
        let found = self.items.iter().any(|item| match *item {
            BracketItem::Char(expected) => expected == c,
            BracketItem::Range(first, last) => (first..=last).contains(&c),
            BracketItem::Class(class) => class.matches(c),
        });
        found != self.negated
    }
}

/// The character at the start of `chars`, `\` quoting the next one, and how
/// many characters it takes.
fn escaped(chars: &[char]) -> Option<(char, usize)> {
    match chars {
        ['\\', c, ..] => Some((*c, 2)),
        [c, ..] => Some((*c, 1)),
        [] => None,
    }
}

impl Class {
    fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "alpha" => Self::Alpha,
            "digit" => Self::Digit,
            "alnum" => Self::Alnum,
            "upper" => Self::Upper,
            "lower" => Self::Lower,
            "space" => Self::Space,
            "blank" => Self::Blank,
            "punct" => Self::Punct,
            "print" => Self::Print,
            "graph" => Self::Graph,
            "cntrl" => Self::Cntrl,
            "xdigit" => Self::Xdigit,
            _ => return None,
        })
    }

    fn matches(self, c: char) -> bool {
        match self {
            Self::Alpha => c.is_alphabetic(),
            Self::Digit => c.is_ascii_digit(),
            Self::Alnum => c.is_alphanumeric(),
            Self::Upper => c.is_uppercase(),
            Self::Lower => c.is_lowercase(),
            Self::Space => c.is_whitespace(),
            Self::Blank => c == ' ' || c == '\t',
            Self::Punct => c.is_ascii_punctuation(),
            Self::Print => !c.is_control(),
            Self::Graph => !c.is_control() && !c.is_whitespace(),
            Self::Cntrl => c.is_control(),
            Self::Xdigit => c.is_ascii_hexdigit(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn matches(pattern: &str, text: &str) -> bool {
        Pattern::new(pattern).matches(text)
    }

    #[test]
    fn star_and_question_mark() {
        assert!(matches("*", ""));
        assert!(matches("*", "abc"));
        assert!(matches("a*c", "abbbc"));
        assert!(matches("a*c", "ac"));
        assert!(!matches("a*c", "abd"));
        assert!(matches("*.rs", "main.rs"));
        assert!(matches("a**b*c*", "axxbyc"));
        assert!(matches("?", "é"));
        assert!(!matches("?", ""));
        assert!(matches("a?c", "abc"));
        assert!(matches("*a*a*a", "aaaa"));
        assert!(!matches("*a*a*a*b", "aaaa"));
    }

    #[test]
    fn brackets() {
        assert!(matches("[abc]", "b"));
        assert!(!matches("[abc]", "d"));
        assert!(matches("[!abc]", "d"));
        assert!(matches("[^abc]", "d"));
        assert!(!matches("[!abc]", "a"));
        assert!(matches("[a-z]x", "qx"));
        assert!(!matches("[a-z]", "Q"));
        assert!(matches("[]a]", "]"));
        assert!(matches("[!]a]", "b"));
        assert!(matches("[a-]", "-"));
        assert!(matches("[[:digit:]][[:alpha:]]", "1é"));
        assert!(matches("[[:upper:][:space:]]", " "));
        assert!(matches("[\\]]", "]"));
    }

    #[test]
    fn invalid_brackets_match_themselves() {
        assert!(matches("[abc", "[abc"));
        assert!(matches("a[", "a["));
        // A bad class: the first `[` is literal, `[:nope:]` is still a
        // bracket expression, then a literal `]`.
        assert!(matches("[[:nope:]]", "[n]"));
    }

    #[test]
    fn backslash_quotes() {
        assert!(matches("\\*", "*"));
        assert!(!matches("\\*", "a"));
        assert!(matches("a\\?", "a?"));
        assert!(matches("\\[a]", "[a]"));
        assert!(matches("a\\", "a\\"));
    }

    #[test]
    fn prefixes_and_suffixes() {
        let star_slash = Pattern::new("*/");
        assert_eq!(star_slash.strip_prefix("a/b/c", false), "b/c");
        assert_eq!(star_slash.strip_prefix("a/b/c", true), "c");

        let dot_star = Pattern::new(".*");
        assert_eq!(dot_star.strip_suffix("a.b.c", false), "a.b");
        assert_eq!(dot_star.strip_suffix("a.b.c", true), "a");

        // No match: unchanged. The empty match: the smallest is nothing.
        assert_eq!(Pattern::new("x").strip_prefix("abc", true), "abc");
        assert_eq!(Pattern::new("*").strip_prefix("abc", false), "abc");
        assert_eq!(Pattern::new("*").strip_suffix("abc", true), "");
        assert_eq!(Pattern::new("?").strip_suffix("città", false), "citt");
    }
}
