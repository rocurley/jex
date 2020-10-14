use serde_json::value::{Number, Value};
use tui::{
    style::{Color, Modifier, Style},
    text::{Span, Spans},
};

#[derive(Debug, Clone)]
pub struct Line {
    pub content: LineContent,
    pub key: Option<Box<str>>,
    pub folded: bool,
    pub indent: u8,
}

impl Line {
    fn is_closing(&self) -> bool {
        match self.content {
            LineContent::ArrayEnd(_) => true,
            LineContent::ObjectEnd(_) => true,
            LineContent::ValueTerminator => true,
            _ => false,
        }
    }
}

pub fn prior_displayable_line(mut i: usize, lines: &[Line]) -> Option<usize> {
    i = i.checked_sub(1)?;
    match &lines[i].content {
        LineContent::ArrayEnd(lines_skipped) | LineContent::ObjectEnd(lines_skipped) => {
            let matching_i = i - 1 - lines_skipped;
            if lines[matching_i].folded {
                Some(matching_i)
            } else {
                Some(i)
            }
        }
        LineContent::ValueTerminator => prior_displayable_line(i, lines),
        _ => Some(i),
    }
}

pub fn next_displayable_line(i: usize, lines: &[Line]) -> Option<usize> {
    let delta = match lines[i] {
        Line {
            content: LineContent::ArrayStart(lines_skipped),
            folded: true,
            ..
        } => lines_skipped + 2,
        Line {
            content: LineContent::ObjectStart(lines_skipped),
            folded: true,
            ..
        } => lines_skipped + 2,
        _ => 1,
    };
    let new_i = i + delta;
    if let LineContent::ValueTerminator = lines.get(new_i)?.content {
        next_displayable_line(new_i, lines)
    } else {
        Some(new_i)
    }
}

#[derive(Debug, Clone)]
pub enum LineContent {
    Null,
    Bool(bool),
    Number(Number),
    String(Box<str>),
    ArrayStart(usize),
    ArrayEnd(usize),
    ObjectStart(usize),
    ObjectEnd(usize),
    ValueTerminator,
}

pub fn json_to_lines<'a, I: Iterator<Item = &'a Value>>(vs: I) -> Vec<Line> {
    let mut out = Vec::new();
    for value in vs {
        json_to_lines_inner(None, value, 0, &mut out);
        push_line(None, LineContent::ValueTerminator, 0, &mut out);
    }
    out
}

fn push_line(key: Option<Box<str>>, content: LineContent, indent: u8, out: &mut Vec<Line>) {
    let line = Line {
        content,
        key,
        folded: false,
        indent,
    };
    out.push(line);
}

fn json_to_lines_inner(key: Option<Box<str>>, v: &Value, indent: u8, out: &mut Vec<Line>) -> usize {
    match v {
        Value::Null => {
            push_line(key, LineContent::Null, indent, out);
            1
        }
        Value::Bool(b) => {
            push_line(key, LineContent::Bool(*b), indent, out);
            1
        }
        Value::Number(x) => {
            push_line(key, LineContent::Number(x.clone()), indent, out);
            1
        }
        Value::String(s) => {
            push_line(key, LineContent::String(s.as_str().into()), indent, out);
            1
        }
        Value::Array(xs) => {
            let mut count = 0;
            let start_position = out.len();
            push_line(key, LineContent::ArrayStart(0), indent, out);
            for x in xs.iter() {
                count += json_to_lines_inner(None, x, indent + 1, out);
            }
            push_line(None, LineContent::ArrayEnd(count), indent, out);
            out[start_position].content = LineContent::ArrayStart(count);
            count + 2
        }
        Value::Object(xs) => {
            let mut count = 0;
            let start_position = out.len();
            push_line(key, LineContent::ObjectStart(0), indent, out);
            for (k, x) in xs.iter() {
                count += json_to_lines_inner(Some(k.as_str().into()), x, indent + 1, out);
            }
            push_line(None, LineContent::ObjectEnd(count), indent, out);
            out[start_position].content = LineContent::ObjectStart(count);
            count + 2
        }
    }
}

pub fn render_lines<'a>(
    scroll: usize,
    line_limit: u16,
    cursor: Option<usize>,
    lines: &'a [Line],
) -> Vec<Spans<'a>> {
    renderable_lines(scroll, lines)
        .take(line_limit as usize)
        .map(|i| render_line(i, cursor, lines))
        .collect()
}

pub fn renderable_lines<'a>(scroll: usize, lines: &'a [Line]) -> impl Iterator<Item = usize> + 'a {
    RenderableLines {
        lines,
        i: if scroll < lines.len() {
            Some(scroll)
        } else {
            None
        },
    }
}

struct RenderableLines<'a> {
    lines: &'a [Line],
    i: Option<usize>,
}

impl<'a> Iterator for RenderableLines<'a> {
    type Item = usize;
    fn next(&mut self) -> Option<usize> {
        let out = self.i;
        self.i = self.i.and_then(|i| next_displayable_line(i, self.lines));
        out
    }
}

fn render_line<'a>(i: usize, cursor: Option<usize>, lines: &'a [Line]) -> Spans<'a> {
    let line = &lines[i];
    let next = lines.get(i + 1);
    let has_comma = match next {
        None => false,
        Some(line) => !line.is_closing(),
    };
    let indent_span = Span::raw("  ".repeat(line.indent as usize));
    let mut out = match &line.key {
        Some(key) => vec![
            indent_span,
            Span::raw(format!("{:?}", key)),
            Span::raw(" : "),
        ],
        _ => vec![indent_span],
    };
    let style = if Some(i) == cursor {
        Style::default().bg(Color::Blue)
    } else {
        Style::default()
    };
    match line {
        Line {
            content: LineContent::ValueTerminator,
            ..
        } => {
            panic!("Shouldn't be trying to render a ValueTerminator");
        }
        Line {
            content: LineContent::Null,
            ..
        } => {
            out.push(Span::styled("null", style));
            if has_comma {
                out.push(Span::raw(","));
            }
        }
        Line {
            content: LineContent::String(s),
            ..
        } => {
            out.push(Span::styled(format!("{:?}", s), style));
            if has_comma {
                out.push(Span::raw(","));
            }
        }
        Line {
            content: LineContent::Bool(b),
            ..
        } => {
            out.push(Span::styled(b.to_string(), style));
            if has_comma {
                out.push(Span::raw(","));
            }
        }
        Line {
            content: LineContent::Number(x),
            ..
        } => {
            out.push(Span::styled(x.to_string(), style));
            if has_comma {
                out.push(Span::raw(","));
            }
        }
        Line {
            content: LineContent::ArrayStart(skipped_lines),
            folded: true,
            ..
        } => {
            out.push(Span::styled("[...]", style));
            if has_comma {
                out.push(Span::raw(","));
            }
            out.push(Span::styled(
                format!(" ({} lines)", skipped_lines),
                Style::default().add_modifier(Modifier::DIM),
            ));
        }
        Line {
            content: LineContent::ArrayEnd(_),
            folded: true,
            ..
        } => {
            panic!("Attempted to print close of folded array");
        }
        Line {
            content: LineContent::ArrayStart(_),
            folded: false,
            ..
        } => {
            out.push(Span::styled("[", style));
        }
        Line {
            content: LineContent::ArrayEnd(_),
            folded: false,
            ..
        } => {
            out.push(Span::styled("]", style));
            if has_comma {
                out.push(Span::raw(","));
            }
        }
        Line {
            content: LineContent::ObjectStart(skipped_lines),
            folded: true,
            ..
        } => {
            out.push(Span::styled("{...}", style));
            if has_comma {
                out.push(Span::raw(","));
            }
            out.push(Span::styled(
                format!(" ({} lines)", skipped_lines),
                Style::default().add_modifier(Modifier::DIM),
            ));
        }
        Line {
            content: LineContent::ObjectEnd(_),
            folded: true,
            ..
        } => {
            panic!("Attempted to print close of folded array");
        }
        Line {
            content: LineContent::ObjectStart(_),
            folded: false,
            ..
        } => {
            out.push(Span::styled("{", style));
        }
        Line {
            content: LineContent::ObjectEnd(_),
            folded: false,
            ..
        } => {
            out.push(Span::styled("}", style));
            if has_comma {
                out.push(Span::raw(","));
            }
        }
    };
    Spans::from(out)
}

#[cfg(feature = "dev-tools")]
mod memory {
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
                let json_size = Value::String(key.to_string()).to_string().as_bytes().len() + 3;
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
                    let json_size = Value::String(s.to_string()).to_string().as_bytes().len() + 2;
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
                ValueTerminator => {
                    self.value_terminator += MemoryStat {
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
            out.value_terminator.json_size -= 1;
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
    use crate::lines::{Line, LineContent};
    use serde_json::value::Number;
    use std::mem::size_of;
    #[test]
    fn test_size() {
        dbg!(size_of::<Line>());
        dbg!(size_of::<LineContent>());
        dbg!(size_of::<Option<String>>());
        dbg!(size_of::<Option<Box<str>>>());
        dbg!(size_of::<usize>());
        dbg!(size_of::<Number>());
        panic!("Print!");
    }
}
