use std::io::Write;
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
    // TODO: something less hilariously inefficient
    pub fn content_width(&self, width: u16) -> u16 {
        let indent_span = Span::raw("  ".repeat(self.indent as usize));
        let mut out = match &self.key {
            Some(key) => vec![
                indent_span,
                Span::raw(format!("\"{}\"", escaped_str(key))),
                Span::raw(" : "),
            ],
            _ => vec![indent_span],
        };
        let consumed_width: u16 = out.iter().map(|span| span.width() as u16).sum();
        let remainining_width = width.saturating_sub(consumed_width);
        remainining_width
    }
    // TODO: wrapping for non-strings (keys???)
    // TODO: Max line count (we don't get any efficiency gain until we have this!)
    pub fn render(self, is_cursor: bool, width: u16, start_byte: usize) -> Vec<Spans<'static>> {
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
                let consumed_width: u16 = out.iter().map(|span| span.width() as u16).sum();
                let remainining_width = width.saturating_sub(consumed_width);
                // TODO: rename this or the other one, it's confusing
                let (mut out, mut escaped_lines) = if start_byte == 0 {
                    let mut escaped_lines = escaped_lines(s, remainining_width);
                    let escaped_line = escaped_lines
                        .next()
                        .expect("escaped_lines must return at least 1 line")
                        .to_string();
                    out.push(Span::styled(escaped_line, style));
                    (vec![Spans::from(out)], escaped_lines)
                } else {
                    (
                        Vec::new(),
                        escaped_lines(&s[start_byte..], remainining_width),
                    )
                };
                for escaped_line in escaped_lines {
                    let padding = Span::raw(" ".repeat(consumed_width as usize));
                    out.push(Spans::from(vec![
                        padding,
                        Span::styled(escaped_line.to_string(), style),
                    ]));
                }
                if self.comma {
                    // TODO: check if the comma will fit
                    out.last_mut()
                        .expect("out cannot be empty")
                        .0
                        .push(Span::raw(","));
                }
                return out;
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
        vec![Spans::from(out)]
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
        | GeneralCategory::SpaceSeparator
        // Combining characters
        | GeneralCategory::SpacingMark
        | GeneralCategory::EnclosingMark
        | GeneralCategory::NonspacingMark => true,
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

pub struct StrLine<'a> {
    pub is_start: bool,
    pub is_end: bool,
    pub raw: &'a str,
    pub start: usize,
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

pub struct LineCursor<'a> {
    pub width: u16,
    pub start: usize,      // bytes
    line_widths: Vec<u16>, //bytes
    current_line: usize,
    pub str: &'a str,
    pub done: bool,
}

impl<'a> LineCursor<'a> {
    fn peek_from(&self, start: usize) -> Option<StrLine<'a>> {
        let is_start = start == 0;
        // open quote
        let width = if is_start { self.width - 1 } else { self.width };
        let rest = &self.str[start..];
        let (raw, width) = take_width(rest, width);
        // Do we need another line with just the close quote?
        let is_end = raw.len() == rest.len() && width > 0;
        return Some(StrLine {
            is_start,
            is_end,
            raw,
            start,
        });
    }
    pub fn peek_prev(&self) -> Option<StrLine<'a>> {
        let line = self.current_line.checked_sub(1)?;
        let start = self.start - self.line_widths[line] as usize;
        self.peek_from(start)
    }
    pub fn peek_next(&self) -> Option<StrLine<'a>> {
        if self.done {
            return None;
        }
        self.peek_from(self.start)
    }
}

fn take_width(s: &str, mut width: u16) -> (&str, u16) {
    for (i, c) in s.char_indices() {
        match width.checked_sub(display_width(c) as u16) {
            None => {
                let raw = &s[..i];
                return (&s[..i], width);
            }
            Some(w) => width = w,
        };
    }
    (s, width)
}

impl<'a> Iterator for LineCursor<'a> {
    type Item = StrLine<'a>;
    fn next(&mut self) -> Option<Self::Item> {
        let next = self.peek_next()?;
        self.done = next.is_end;
        self.start += next.raw.len();
        match self.line_widths.len().cmp(&self.current_line) {
            std::cmp::Ordering::Less => panic!("line_widths hasn't been maintained"),
            std::cmp::Ordering::Equal => self.line_widths.push(next.raw.len() as u16),
            std::cmp::Ordering::Greater => {}
        }
        self.current_line += 1;
        Some(next)
    }
}

fn escaped_lines<'a>(str: &'a str, width: u16) -> LineCursor<'a> {
    LineCursor {
        width,
        start: 0,
        line_widths: Vec::new(),
        current_line: 0,
        str,
        done: false,
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
