use tui::{
    style::{Color, Modifier, Style},
    text::{Span, Spans},
};
use unicode_general_category::{get_general_category, GeneralCategory};
use unicode_width::UnicodeWidthChar;

#[derive(Debug, Clone, PartialEq)]
pub struct Line<'a> {
    pub content: LineContent<'a>,
    pub key: Option<&'a str>,
    pub indent: u8,
    pub comma: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum LineContent<'a> {
    Null,
    Bool(bool),
    Number(f64),
    String(&'a str),
    FoldedArray(usize),
    ArrayStart,
    ArrayEnd,
    FoldedObject(usize),
    ObjectStart,
    ObjectEnd,
}

use std::fmt::Debug;
impl<'a> Line<'a> {
    pub fn render(self, is_cursor: bool) -> Spans<'static> {
        let indent_span = Span::raw("  ".repeat(self.indent as usize));
        let mut out = match &self.key {
            Some(key) => vec![
                indent_span,
                Span::raw(format!("\"{}\"", escaped_str(key))),
                Span::raw(" : "),
            ],
            _ => vec![indent_span],
        };
        let style = if is_cursor {
            Style::default().bg(Color::Blue)
        } else {
            Style::default()
        };
        match self.content {
            LineContent::Null => {
                out.push(Span::styled("null", style));
                if self.comma {
                    out.push(Span::raw(","));
                }
            }
            LineContent::String(s) => {
                out.push(Span::styled(format!("\"{}\"", escaped_str(s)), style));
                if self.comma {
                    out.push(Span::raw(","));
                }
            }
            LineContent::Bool(b) => {
                out.push(Span::styled(b.to_string(), style));
                if self.comma {
                    out.push(Span::raw(","));
                }
            }
            LineContent::Number(x) => {
                out.push(Span::styled(x.to_string(), style));
                if self.comma {
                    out.push(Span::raw(","));
                }
            }
            LineContent::FoldedArray(children) => {
                out.push(Span::styled("[...]", style));
                if self.comma {
                    out.push(Span::raw(","));
                }
                out.push(Span::styled(
                    format!(" ({} children)", children),
                    Style::default().add_modifier(Modifier::DIM),
                ));
            }
            LineContent::ArrayStart => {
                out.push(Span::styled("[", style));
            }
            LineContent::ArrayEnd => {
                out.push(Span::styled("]", style));
                if self.comma {
                    out.push(Span::raw(","));
                }
            }
            LineContent::FoldedObject(children) => {
                out.push(Span::styled("{...}", style));
                if self.comma {
                    out.push(Span::raw(","));
                }
                out.push(Span::styled(
                    format!(" ({} children)", children),
                    Style::default().add_modifier(Modifier::DIM),
                ));
            }
            LineContent::ObjectStart => {
                out.push(Span::styled("{", style));
            }
            LineContent::ObjectEnd => {
                out.push(Span::styled("}", style));
                if self.comma {
                    out.push(Span::raw(","));
                }
            }
        };
        Spans::from(out)
    }
}

fn is_unicode_escaped(c: char) -> bool {
    match get_general_category(c) {
        GeneralCategory::Control
        | GeneralCategory::Format
        | GeneralCategory::Surrogate
        | GeneralCategory::PrivateUse
        | GeneralCategory::LineSeparator
        | GeneralCategory::ParagraphSeparator
        | GeneralCategory::SpaceSeparator => true,
        _ => false,
    }
}

fn escaped_str(s: &str) -> String {
    let mut escaped_raw = Vec::new();
    write_escaped_str(s, &mut escaped_raw).expect("Writing to a vector should be infaliable");
    String::from_utf8(escaped_raw).expect("Escaped string was not utf-8")
}

// TODO: Consider an optimized version of this that writes sequences of unescaped characters in one
// go.
fn write_escaped_str<W: std::io::Write>(s: &str, w: &mut W) -> std::io::Result<()> {
    for c in s.chars() {
        write_escaped_char(c, w)?;
    }
    Ok(())
}

fn write_escaped_char<W: std::io::Write>(c: char, w: &mut W) -> std::io::Result<()> {
    match c {
        '\"' => write!(w, r#"\""#),
        '\\' => write!(w, r#"\\"#),
        '\u{08}' => write!(w, r#"\b"#),
        '\u{0C}' => write!(w, r#"\f"#),
        '\n' => write!(w, r#"\n"#),
        '\r' => write!(w, r#"\r"#),
        '\t' => write!(w, r#"\t"#),
        _ if is_unicode_escaped(c) => write!(w, "\\u{:04x}", c as u32), // \u1234
        _ => write!(w, "{}", c),
    }
}

fn display_width(c: char) -> u8 {
    match c {
        '\"' | '\\' | '\u{08}' | '\u{0C}' | '\n' | '\r' | '\t' => 2,
        _ if is_unicode_escaped(c) => 6, // \u1234
        // TODO: It kind of sucks to have this huge table that get_general_category uses and
        // not even get the width from it. Probably we should make our own table at some point,
        // with values Escaped | HalfWidth | FullWidth | Special. 2 bits, you could pack that in
        // pretty nicely.
        _ => c
            .width()
            .expect("control characters should have been filtered out above") as u8,
    }
}

struct StrLine<'a> {
    is_start: bool,
    is_end: bool,
    raw: &'a str,
}

impl<'a> StrLine<'a> {
    fn to_string(&self) -> String {
        let mut escaped_raw = Vec::new();
        if self.is_start {
            write!(&mut escaped_raw, "\"");
        }
        write_escaped_str(self.raw, &mut escaped_raw);
        if self.is_end {
            write!(&mut escaped_raw, "\"");
        }
        String::from_utf8(escaped_raw).expect("Escaped string was not utf-8")
    }
}

struct StrLineIter<'a> {
    width: u8,
    is_start: bool,
    rest: &'a str,
    done: bool,
}

impl<'a> Iterator for StrLineIter<'a> {
    type Item = StrLine<'a>;
    fn next(&mut self) -> Option<Self::Item> {
        let is_start = self.is_start;
        self.is_start = false;
        let mut width = if is_start { self.width - 1 } else { self.width };
        let mut chars = self.rest.char_indices();
        loop {
            let (i, c) = match chars.next() {
                None => {
                    let raw = self.rest;
                    self.rest = "";
                    // Do we need another line with just the close quote?
                    self.done = width > 0;
                    return Some(StrLine {
                        is_start,
                        is_end: self.done,
                        raw,
                    });
                }
                Some(pair) => pair,
            };
            match width.checked_sub(display_width(c)) {
                None => {
                    let raw = &self.rest[..i];
                    self.rest = &self.rest[i..];
                    return Some(StrLine {
                        is_start,
                        is_end: false,
                        raw,
                    });
                }
                Some(w) => width = w,
            };
        }
    }
}

#[cfg(feature = "dev-tools")]
pub mod memory {
    use super::{Line, LineContent};
    use serde_json::Value;
    #[derive(Debug, Clone, Default)]
    pub struct MemoryStats {
        pub null: MemoryStat,
        pub bool: MemoryStat,
        pub number: MemoryStat,
        pub string: MemoryStat,
        pub array_start: MemoryStat,
        pub array_end: MemoryStat,
        pub object_start: MemoryStat,
        pub object_end: MemoryStat,
        pub value_terminator: MemoryStat,

        pub key: MemoryStat,
    }

    impl MemoryStats {
        pub fn new() -> Self {
            Self::default()
        }
        pub fn log(&mut self, l: &Line) {
            if let Some(key) = &l.key {
                let json_size = Value::String(key.to_string()).to_string().as_bytes().len() + 1;
                self.key += MemoryStat {
                    count: 0,
                    json_size,
                    indirect_bytes: key.as_bytes().len(),
                }
            }
            use LineContent::*;
            match &l.content {
                Null => {
                    self.null += MemoryStat {
                        count: 1,
                        json_size: 4,
                        indirect_bytes: 0,
                    }
                }
                Bool(b) => {
                    let json_size = if *b { 4 } else { 5 };
                    self.bool += MemoryStat {
                        count: 1,
                        json_size,
                        indirect_bytes: 0,
                    }
                }
                Number(n) => {
                    let json_size = n.to_string().len();
                    self.number += MemoryStat {
                        count: 1,
                        json_size,
                        indirect_bytes: 0,
                    }
                }
                String(s) => {
                    let json_size = Value::String(s.to_string()).to_string().as_bytes().len();
                    self.string += MemoryStat {
                        count: 1,
                        json_size,
                        indirect_bytes: s.as_bytes().len(),
                    }
                }
                ArrayStart(_) => {
                    self.array_start += MemoryStat {
                        count: 1,
                        json_size: 1,
                        indirect_bytes: 0,
                    }
                }
                ArrayEnd(_) => {
                    self.array_end += MemoryStat {
                        count: 1,
                        json_size: 1,
                        indirect_bytes: 0,
                    }
                }
                ObjectStart(_) => {
                    self.object_start += MemoryStat {
                        count: 1,
                        json_size: 1,
                        indirect_bytes: 0,
                    }
                }
                ObjectEnd(_) => {
                    self.object_end += MemoryStat {
                        count: 1,
                        json_size: 1,
                        indirect_bytes: 0,
                    }
                }
            }
        }
        pub fn from_lines(lines: &[Line]) -> Self {
            let mut out = Self::new();
            for line in lines {
                out.log(line)
            }
            out
        }
    }

    #[derive(Debug, Clone, Default, Copy)]
    pub struct MemoryStat {
        pub count: usize,
        pub json_size: usize,
        pub indirect_bytes: usize,
    }
    impl std::ops::AddAssign for MemoryStat {
        fn add_assign(&mut self, other: Self) {
            self.count += other.count;
            self.json_size += other.json_size;
            self.indirect_bytes += other.indirect_bytes;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{display_width, escaped_str};
    use proptest::prelude::*;
    use unicode_width::UnicodeWidthStr;
    proptest! {
        #[test]
        fn prop_display_width(string in any::<String>()) {
            let escaped = escaped_str(&string);
            let expected_width = escaped.width();
            let actual_inner_width: usize = string.chars().map(|c| display_width(c) as usize).sum();
            assert_eq!(expected_width, actual_inner_width , "original: {:?}, escaped: {}", &string, &escaped);
        }
    }
}
