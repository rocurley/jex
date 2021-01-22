use crate::jq::jv::JVString;
use std::{cell::RefCell, io::Write, rc::Rc};
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
    pub indent: u16,
    pub comma: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum LineContent<'a> {
    Null,
    Bool(bool),
    Number(f64),
    String(StrLine<'a>),
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
    // TODO: wrapping for non-strings (keys???)
    pub fn render(self, is_cursor: bool, width: u16) -> Spans<'static> {
        let indent_span = Span::raw(" ".repeat(self.indent as usize));
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
            LineContent::String(str_line) => {
                let quoted = str_line.to_string();
                out.push(Span::styled(quoted, style));
                if self.comma {
                    // TODO: check if the comma will fit
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
        // Combining characters
        | GeneralCategory::SpacingMark
        | GeneralCategory::EnclosingMark
        | GeneralCategory::NonspacingMark => true,
        GeneralCategory::SpaceSeparator => c != ' ',
        _ => false,
    }
}

pub fn escaped_str(s: &str) -> String {
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
        _ if is_unicode_escaped(c) => {
            let mut buf = [0u16, 0];
            let encoded = c.encode_utf16(&mut buf);
            for pt in encoded {
                write!(w, "\\u{:04x}", *pt)?; // \u1234
            }
            Ok(())
        }
        _ => write!(w, "{}", c),
    }
}

fn display_width(c: char) -> u8 {
    match c {
        '\"' | '\\' | '\u{08}' | '\u{0C}' | '\n' | '\r' | '\t' => 2,
        _ if is_unicode_escaped(c) => 6 * c.len_utf16() as u8, // \u1234
        // TODO: It kind of sucks to have this huge table that get_general_category uses and
        // not even get the width from it. Probably we should make our own table at some point,
        // with values Escaped | HalfWidth | FullWidth | Special. 2 bits, you could pack that in
        // pretty nicely.
        _ => c
            .width()
            .expect("control characters should have been filtered out above") as u8,
    }
}

#[derive(Debug, Clone, PartialEq)]
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
            write!(&mut escaped_raw, "\"").expect("Write to a vec should be infaliable");
        }
        write_escaped_str(self.raw, &mut escaped_raw).expect("Write to a vec should be infaliable");
        if self.is_end {
            write!(&mut escaped_raw, "\"").expect("Write to a vec should be infaliable");
        }
        String::from_utf8(escaped_raw).expect("Escaped string was not utf-8")
    }
}

#[derive(PartialEq, Eq, Debug, Clone, Copy)]
enum LineCursorPosition {
    Start,
    End,
    Valid {
        start: usize, // bytes
        current_line: usize,
    },
}

#[derive(PartialEq, Eq, Debug, Clone)]
pub struct LineCursor {
    width: u16,
    line_widths: Rc<RefCell<Vec<u16>>>, //bytes
    position: LineCursorPosition,
    value: JVString,
}

impl LineCursor {
    pub fn current(&self) -> Option<StrLine> {
        match self.position {
            LineCursorPosition::Start | LineCursorPosition::End => None,
            LineCursorPosition::Valid {
                start,
                current_line,
            } => {
                let is_start = start == 0;
                let line_widths = self.line_widths.borrow();
                let end = start + line_widths[current_line] as usize;
                // We guarantee that we'll push an empty line to line_widths if we scroll to the
                // end and there's no room for a closing quote.
                let is_end =
                    self.value.value().len() == end && current_line == line_widths.len() - 1;
                let raw = &self.value.value()[start..end];
                Some(StrLine {
                    is_start,
                    is_end,
                    raw,
                    start,
                })
            }
        }
    }
    fn extend_line_widths(line_widths: &mut Vec<u16>, s: &str, width: u16) {
        let (line, line_term_width) = take_width(s, width);
        if s.len() > 0 {
            assert!(
                line.len() != 0,
                "Took zero-width line for string {:?} with width {}",
                s,
                width
            );
        }
        line_widths.push(line.len() as u16);
        if line.len() == s.len() && line_term_width == width {
            // Everything but the closing quote fits on this line
            line_widths.push(0);
        };
    }
    pub fn move_next(&mut self) {
        let mut line_widths = self.line_widths.borrow_mut();
        match &mut self.position {
            LineCursorPosition::Start => {
                if line_widths.is_empty() {
                    // width - 1 for the opening quote
                    Self::extend_line_widths(
                        line_widths.as_mut(),
                        self.value.value(),
                        self.width - 1,
                    );
                }
                self.position = LineCursorPosition::Valid {
                    current_line: 0,
                    start: 0,
                }
            }
            LineCursorPosition::End => {}
            LineCursorPosition::Valid {
                current_line,
                start,
            } => {
                *start += line_widths[*current_line] as usize;
                *current_line += 1;
                if *current_line == line_widths.len() {
                    let s = self.value.value();
                    assert!(*start <= s.len());
                    if *start == s.len() {
                        self.position = LineCursorPosition::End;
                    } else {
                        Self::extend_line_widths(&mut line_widths, &s[*start..], self.width);
                    }
                }
            }
        }
    }
    pub fn move_prev(&mut self) {
        let line_widths = self.line_widths.borrow_mut();
        match &mut self.position {
            LineCursorPosition::Start => {}
            LineCursorPosition::End => {
                let current_line = line_widths.len() - 1;
                let s = self.value.value();
                let start = s.len() - line_widths[current_line] as usize;
                self.position = LineCursorPosition::Valid {
                    current_line,
                    start,
                }
            }
            LineCursorPosition::Valid { current_line, .. } if *current_line == 0 => {
                self.position = LineCursorPosition::Start
            }
            LineCursorPosition::Valid {
                current_line,
                start,
            } => {
                *current_line -= 1;
                *start -= line_widths[*current_line] as usize;
            }
        }
    }
    pub fn current_line(&self) -> Option<usize> {
        match self.position {
            LineCursorPosition::Valid { current_line, .. } => Some(current_line),
            LineCursorPosition::Start | LineCursorPosition::End => None,
        }
    }
    pub fn new_at_start(value: JVString, width: u16) -> Self {
        assert!(width > 6);
        let mut out = LineCursor {
            line_widths: Rc::new(RefCell::new(Vec::new())),
            position: LineCursorPosition::Start,
            value,
            width,
        };
        out.move_next();
        out
    }
    pub fn new_at_end(value: JVString, width: u16) -> Self {
        assert!(width > 6);
        // We start from the start and scan forward to populate line_widths
        let mut out = Self::new_at_start(value, width);
        while out.position != LineCursorPosition::End {
            out.move_next();
        }
        out.move_prev();
        out
    }
    pub fn set_width(&mut self, width: u16) {
        assert_eq!(self.width, width);
    }
}

fn take_width(s: &str, target_width: u16) -> (&str, u16) {
    let mut width = 0u16;
    for (i, c) in s.char_indices() {
        let new_width = width + display_width(c) as u16;
        if new_width > target_width {
            let raw = &s[..i];
            return (&s[..i], width);
        }
        width = new_width;
    }
    (s, width)
}

#[cfg(test)]
mod tests {
    use super::{display_width, escaped_str, LineCursor, StrLine};
    use crate::jq::jv::JVString;
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
    fn read_cursor_lines_reverse(mut cursor: LineCursor) -> String {
        let mut out = String::new();
        while let Some(line) = cursor.current() {
            let mut s = line.to_string();
            assert!(s.width() <= cursor.width as usize);
            std::mem::swap(&mut out, &mut s);
            out.extend(s.chars());
            cursor.move_prev();
        }
        out
    }
    fn read_cursor_lines(mut cursor: LineCursor) -> String {
        let mut out = String::new();
        while let Some(line) = cursor.current() {
            let s = line.to_string();
            assert!(s.width() <= cursor.width as usize);
            out.extend(s.chars());
            cursor.move_next();
        }
        out
    }
    fn check_lines(string: &str, width: u16) {
        let value = JVString::new(&string);
        {
            let wide_cursor = LineCursor::new_at_start(value.clone(), u16::MAX);
            let actual_cursor = LineCursor::new_at_start(value.clone(), width);
            let expected = read_cursor_lines(wide_cursor);
            let actual = read_cursor_lines(actual_cursor);
            assert!(actual.len() >= 2);
            assert_eq!(expected, actual);
        }
        {
            let wide_cursor = LineCursor::new_at_end(value.clone(), u16::MAX);
            let actual_cursor = LineCursor::new_at_end(value, width);
            let expected = read_cursor_lines_reverse(wide_cursor);
            let actual = read_cursor_lines_reverse(actual_cursor);
            assert!(actual.len() >= 2);
            assert_eq!(expected, actual);
        }
    }
    proptest! {
        #[test]
        fn prop_display_lines(string in any::<String>(), width in 7..u16::MAX) {
            check_lines(&string, width);
        }
    }
    #[test]
    fn unit_display_lines() {
        let tests = vec![
            ("aaa\u{e000}¡", 8),
            ("\u{0}\u{0}\u{7f}\u{3fffe}®\u{e000}A0\u{3fffe}𠀀\"", 8),
        ];
        for (string, width) in tests {
            check_lines(&string, width);
        }
    }
    #[test]
    fn unit_to_string() {
        let tests = vec![
            ("", r#""""#),
            ("Hello world!", r#""Hello world!""#),
            ("Hello\nworld!", r#""Hello\nworld!""#),
        ];
        for (raw, expected) in tests {
            let line = StrLine {
                is_start: true,
                is_end: true,
                raw,
                start: 0,
            };
            assert_eq!(line.to_string(), expected, "Test failure for {:?}", raw);
        }
    }
}
