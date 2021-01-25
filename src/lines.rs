use crate::jq::jv::JVString;
use std::{cell::RefCell, io::Write, ops::Range, rc::Rc};
use tui::{
    style::{Color, Modifier, Style},
    text::{Span, Spans},
};
use unicode_general_category::{get_general_category, GeneralCategory};
use unicode_width::UnicodeWidthChar;

#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub content: LineContent,
    pub key: Option<JVString>,
    pub indent: u16,
    pub comma: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum LineContent {
    Null,
    Bool(bool),
    Number(f64),
    String(JVString),
    FoldedArray(usize),
    ArrayStart,
    ArrayEnd,
    FoldedObject(usize),
    ObjectStart,
    ObjectEnd,
}

use std::fmt::Debug;
impl Line {
    pub fn render(self, is_cursor: bool) -> LineFragments {
        let indent = LineFragment::new(" ".repeat(self.indent as usize), false, Style::default());
        let style = if is_cursor {
            Style::default().bg(Color::Blue)
        } else {
            Style::default()
        };
        let mut out = match self.key {
            Some(key) => vec![
                indent,
                LineFragment::new("\"", false, Style::default()),
                LineFragment::new(key, true, Style::default()),
                LineFragment::new("\" : ", false, Style::default()),
            ],
            _ => vec![indent],
        };
        match self.content {
            LineContent::Null => {
                out.push(LineFragment::new("null", false, style));
                if self.comma {
                    out.push(LineFragment::new_unstyled(",", false));
                }
            }
            LineContent::String(string) => {
                out.push(LineFragment::new(string, true, style));
                if self.comma {
                    out.push(LineFragment::new_unstyled(",", false));
                }
            }
            LineContent::Bool(b) => {
                out.push(LineFragment::new(b.to_string(), false, style));
                if self.comma {
                    out.push(LineFragment::new_unstyled(",", false));
                }
            }
            LineContent::Number(x) => {
                out.push(LineFragment::new(x.to_string(), false, style));
                if self.comma {
                    out.push(LineFragment::new_unstyled(",", false));
                }
            }
            LineContent::FoldedArray(children) => {
                out.push(LineFragment::new("[...]", false, style));
                if self.comma {
                    out.push(LineFragment::new_unstyled(",", false));
                }
                out.push(LineFragment::new(
                    format!(" ({} children)", children),
                    false,
                    Style::default().add_modifier(Modifier::DIM),
                ));
            }
            LineContent::ArrayStart => {
                out.push(LineFragment::new("[", false, style));
            }
            LineContent::ArrayEnd => {
                out.push(LineFragment::new("]", false, style));
                if self.comma {
                    out.push(LineFragment::new_unstyled(",", false));
                }
            }
            LineContent::FoldedObject(children) => {
                out.push(LineFragment::new("{...}", false, style));
                if self.comma {
                    out.push(LineFragment::new_unstyled(",", false));
                }
                out.push(LineFragment::new(
                    format!(" ({} children)", children),
                    false,
                    Style::default().add_modifier(Modifier::DIM),
                ));
            }
            LineContent::ObjectStart => {
                out.push(LineFragment::new("{", false, style));
            }
            LineContent::ObjectEnd => {
                out.push(LineFragment::new("}", false, style));
                if self.comma {
                    out.push(LineFragment::new_unstyled(",", false));
                }
            }
        };
        LineFragments::new(out)
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
pub struct StrLine {
    pub is_start: bool,
    pub is_end: bool,
    pub content: Spans<'static>,
}

#[derive(PartialEq, Eq, Debug, Clone, Copy, PartialOrd, Ord)]
enum LineCursorPosition {
    Start,
    Valid {
        start: LineFragmentsIndex,
        current_line: usize,
    },
    End,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum StringLike {
    Constant(&'static str),
    String(String),
    JV(JVString),
}

impl StringLike {
    fn as_str(&self) -> &str {
        match self {
            StringLike::Constant(s) => s,
            StringLike::String(s) => s.as_str(),
            StringLike::JV(s) => s.value(),
        }
    }
    fn len(&self) -> usize {
        self.as_str().len()
    }
}

impl From<&'static str> for StringLike {
    fn from(s: &'static str) -> Self {
        StringLike::Constant(s)
    }
}

impl From<String> for StringLike {
    fn from(s: String) -> Self {
        StringLike::String(s)
    }
}

impl From<JVString> for StringLike {
    fn from(s: JVString) -> Self {
        StringLike::JV(s)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct LineFragment {
    string: StringLike,
    is_escaped: bool,
    style: Style,
}

impl LineFragment {
    fn new<S: Into<StringLike>>(s: S, is_escaped: bool, style: Style) -> Self {
        LineFragment {
            string: s.into(),
            is_escaped,
            style,
        }
    }
    fn new_unstyled<S: Into<StringLike>>(s: S, is_escaped: bool) -> Self {
        LineFragment {
            string: s.into(),
            is_escaped,
            style: Style::default(),
        }
    }
    fn take_width(&self, from: usize, target_width: u16) -> (Range<usize>, u16) {
        if self.is_escaped {
            let mut width = 0u16;
            for (i, c) in self.string.as_str()[from..].char_indices() {
                let new_width = width + display_width(c) as u16;
                if new_width > target_width {
                    return (from..from + i, width);
                }
                width = new_width;
            }
            (from..self.string.len(), width)
        } else {
            let width = std::cmp::min(self.string.len() - from, target_width as usize);
            (from..from + width, width as u16)
        }
    }
    fn span(&self, range: Range<usize>) -> Span<'static> {
        let s = if self.is_escaped {
            escaped_str(&self.string.as_str()[range])
        } else {
            self.string.as_str().to_string()
        };
        Span::styled(s, self.style)
    }
}

#[derive(Clone, Debug)]
pub struct LineFragments(Vec<LineFragment>);

#[derive(Clone, Copy, Debug, Eq, PartialEq, PartialOrd, Ord)]
pub struct LineFragmentsIndex {
    fragment_index: usize,
    byte_index: usize,
}

impl LineFragments {
    fn new(v: Vec<LineFragment>) -> Self {
        Self(v)
    }
    fn take_width(
        &self,
        from: LineFragmentsIndex,
        target_width: u16,
    ) -> (Range<LineFragmentsIndex>, u16) {
        let mut current = from;
        let width = 0;
        loop {
            let fragment = self.0[current.fragment_index];
            let (fragment_range, fragment_width) =
                fragment.take_width(current.byte_index, target_width - width);
            width += fragment_width;
            current.byte_index = fragment_range.end;
            if fragment_range.end != fragment.string.len() {
                // Didn't consume the whole fragment
                break;
            }
            if width == target_width {
                // Out of width
                break;
            }
            if current.fragment_index == self.0.len() {
                // No more fragments
                break;
            }
            current.fragment_index += 1;
            current.byte_index = 0;
        }
        (from..current, width)
    }
    fn spans(&self, range: Range<LineFragmentsIndex>) -> Spans<'static> {
        self.0
            .iter()
            .enumerate()
            .map(|(i, fragment)| {
                let start = if i == 0 { range.start.byte_index } else { 0 };
                let end = if i == range.end.fragment_index - range.start.fragment_index {
                    range.end.byte_index
                } else {
                    fragment.string.len()
                };
                fragment.span(start..end)
            })
            .collect::<Vec<_>>()
            .into()
    }
    fn to_global_byte_offset(&self, ix: LineFragmentsIndex) -> usize {
        self.0
            .iter()
            .take(ix.fragment_index)
            .map(|fragment| fragment.string.len())
            .sum::<usize>()
            + ix.byte_index
    }
    fn from_global_byte_offset(&self, mut offset: usize) -> LineFragmentsIndex {
        for (fragment_index, fragment) in self.0.iter().enumerate() {
            if offset <= fragment.string.len() {
                return LineFragmentsIndex {
                    fragment_index,
                    byte_index: offset,
                };
            }
            offset -= fragment.string.len();
        }
        panic!("Offset out of bounds")
    }
    fn add_byte_offset(&self, mut ix: LineFragmentsIndex, delta: usize) -> LineFragmentsIndex {
        ix.byte_index += delta;
        while ix.byte_index >= self.0[ix.fragment_index].string.len() {
            ix.byte_index -= self.0[ix.fragment_index].string.len();
        }
        ix
    }
    fn sub_byte_offset(&self, mut ix: LineFragmentsIndex, mut delta: usize) -> LineFragmentsIndex {
        while delta > ix.byte_index {
            delta -= ix.byte_index - 1;
            ix.fragment_index -= 1;
            ix.byte_index = self.0[ix.fragment_index].string.len() - 1;
        }
        ix.byte_index -= delta;
        ix
    }
    fn end_index(&self) -> LineFragmentsIndex {
        LineFragmentsIndex {
            fragment_index: self.0.len(),
            byte_index: self.0.last().unwrap().string.len(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct LineCursor {
    width: u16,
    line_widths: Rc<RefCell<Vec<u16>>>, //bytes
    position: LineCursorPosition,
    content: LineFragments,
}

impl LineCursor {
    pub fn current(&self) -> Option<StrLine> {
        match self.position {
            LineCursorPosition::Start | LineCursorPosition::End => None,
            LineCursorPosition::Valid {
                start,
                current_line,
            } => {
                let is_start = start.fragment_index == 0 && start.byte_index == 0;
                let line_widths = self.line_widths.borrow();
                let end = self
                    .content
                    .add_byte_offset(start, line_widths[current_line] as usize);
                let is_end = self.content.end_index() == end;
                let content = self.content.spans(start..end);
                Some(StrLine {
                    is_start,
                    is_end,
                    content,
                })
            }
        }
    }
    fn push_next_line_width(&mut self) {
        let mut line_widths = self.line_widths.borrow_mut();
        match self.position {
            LineCursorPosition::Start => {
                if line_widths.is_empty() {
                    let (range, _) = self.content.take_width(
                        LineFragmentsIndex {
                            fragment_index: 0,
                            byte_index: 0,
                        },
                        self.width,
                    );
                    line_widths.push(self.content.to_global_byte_offset(range.end) as u16);
                }
            }
            LineCursorPosition::End => {}
            LineCursorPosition::Valid {
                current_line,
                start,
            } => {
                if current_line == line_widths.len() {
                    let (range, _) = self.content.take_width(start, self.width);
                    line_widths.push(
                        (self.content.to_global_byte_offset(range.end)
                            - self.content.to_global_byte_offset(start))
                            as u16,
                    );
                }
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
        match &mut self.position {
            LineCursorPosition::Start => {
                self.position = LineCursorPosition::Valid {
                    current_line: 0,
                    start: LineFragmentsIndex {
                        fragment_index: 0,
                        byte_index: 0,
                    },
                };
            }
            LineCursorPosition::End => {}
            LineCursorPosition::Valid {
                current_line,
                start,
            } => {
                *start = self
                    .content
                    .add_byte_offset(*start, self.line_widths.borrow()[*current_line] as usize);
                *current_line += 1;
            }
        }
        self.push_next_line_width();
    }
    pub fn move_prev(&mut self) {
        let line_widths = self.line_widths.borrow_mut();
        match &mut self.position {
            LineCursorPosition::Start => {}
            LineCursorPosition::End => {
                let current_line = line_widths.len() - 1;
                let start = self
                    .content
                    .sub_byte_offset(self.content.end_index(), line_widths[current_line] as usize);
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
                *start = self
                    .content
                    .sub_byte_offset(*start, line_widths[*current_line] as usize);
            }
        }
    }
    pub fn current_line(&self) -> Option<usize> {
        match self.position {
            LineCursorPosition::Valid { current_line, .. } => Some(current_line),
            LineCursorPosition::Start | LineCursorPosition::End => None,
        }
    }
    pub fn new_at_start(content: LineFragments, width: u16) -> Self {
        assert!(width > 6);
        let mut out = LineCursor {
            line_widths: Rc::new(RefCell::new(Vec::new())),
            position: LineCursorPosition::Start,
            content,
            width,
        };
        out.move_next();
        out
    }
    pub fn new_at_end(content: LineFragments, width: u16) -> Self {
        assert!(width > 6);
        // We start from the start and scan forward to populate line_widths
        let mut out = Self::new_at_start(content, width);
        while out.position != LineCursorPosition::End {
            out.move_next();
        }
        out.move_prev();
        out
    }
    pub fn set_width(&mut self, width: u16) {
        if self.width == width {
            return;
        }
        match self.position {
            LineCursorPosition::Start => {
                *self = LineCursor::new_at_start(self.content.clone(), width);
                self.move_prev();
            }
            LineCursorPosition::End => {
                *self = LineCursor::new_at_end(self.content.clone(), width);
                self.move_next();
            }
            LineCursorPosition::Valid { start: target, .. } => {
                *self = LineCursor::new_at_start(self.content.clone(), width);
                loop {
                    match self.position {
                        LineCursorPosition::Start => {
                            panic!("Shouldn't be able to reach start by advancing")
                        }
                        LineCursorPosition::End => break,
                        LineCursorPosition::Valid { start, .. } => {
                            if start > target {
                                break;
                            }
                            self.move_next();
                        }
                    }
                }
                self.move_prev();
            }
        }
    }
}

fn take_width(s: &str, target_width: u16) -> (&str, u16) {
    let mut width = 0u16;
    for (i, c) in s.char_indices() {
        let new_width = width + display_width(c) as u16;
        if new_width > target_width {
            return (&s[..i], width);
        }
        width = new_width;
    }
    (s, width)
}

#[cfg(test)]
mod tests {
    use super::{display_width, escaped_str, LineCursor, StrLine, StringLike};
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
            let wide_cursor = LineCursor::new_at_start(StringLike::JV(value.clone()), u16::MAX);
            let actual_cursor = LineCursor::new_at_start(StringLike::JV(value.clone()), width);
            let expected = read_cursor_lines(wide_cursor);
            let actual = read_cursor_lines(actual_cursor);
            assert!(actual.len() >= 2);
            assert_eq!(expected, actual);
        }
        {
            let wide_cursor = LineCursor::new_at_end(StringLike::JV(value.clone()), u16::MAX);
            let actual_cursor = LineCursor::new_at_end(StringLike::JV(value), width);
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
