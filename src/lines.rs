use serde_json::value::{Number, Value};
use tui::{
    style::{Color, Modifier, Style},
    text::Span,
};

#[derive(Debug, Clone)]
pub struct Line {
    pub content: LineContent,
    pub key: Option<String>,
    pub folded: bool,
}

impl Line {
    fn is_closing(&self) -> bool {
        match self.content {
            LineContent::ArrayStart(_) => true,
            LineContent::ObjectEnd(_) => true,
            _ => false,
        }
    }
    fn next_displayed_offset(&self) -> usize {
        match self {
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
        }
    }
}

#[derive(Debug, Clone)]
pub enum LineContent {
    Null,
    Bool(bool),
    Number(Number),
    String(String),
    ArrayStart(usize),
    ArrayEnd(usize),
    ObjectStart(usize),
    ObjectEnd(usize),
}

pub fn json_to_lines<'a, I: Iterator<Item = &'a Value>>(vs: I) -> Vec<Vec<Line>> {
    vs.into_iter()
        .map(|value| {
            let mut out = Vec::new();
            json_to_lines_inner(None, value, &mut out);
            out
        })
        .collect()
}

fn push_line(key: Option<String>, content: LineContent, out: &mut Vec<Line>) {
    let line = Line {
        content,
        key,
        folded: false,
    };
    out.push(line);
}

fn json_to_lines_inner(key: Option<String>, v: &Value, out: &mut Vec<Line>) -> usize {
    match v {
        Value::Null => {
            push_line(key, LineContent::Null, out);
            1
        }
        Value::Bool(b) => {
            push_line(key, LineContent::Bool(*b), out);
            1
        }
        Value::Number(x) => {
            push_line(key, LineContent::Number(x.clone()), out);
            1
        }
        Value::String(s) => {
            push_line(key, LineContent::String(s.clone()), out);
            1
        }
        Value::Array(xs) => {
            let mut count = 0;
            let start_position = out.len();
            push_line(key, LineContent::ArrayStart(0), out);
            for x in xs.iter() {
                count += json_to_lines_inner(None, x, out);
            }
            push_line(None, LineContent::ArrayEnd(count), out);
            out[start_position].content = LineContent::ArrayStart(count);
            count + 2
        }
        Value::Object(xs) => {
            let mut count = 0;
            let start_position = out.len();
            push_line(key, LineContent::ArrayStart(0), out);
            for (k, x) in xs.iter() {
                count += json_to_lines_inner(Some(k.clone()), x, out);
            }
            push_line(None, LineContent::ArrayEnd(count), out);
            out[start_position].content = LineContent::ArrayStart(count);
            count + 2
        }
    }
}

struct JsonText<'a> {
    indent: usize,
    lines: &'a [Line],
    cursor: usize,
    i: usize,
}
impl<'a> Iterator for JsonText<'a> {
    type Item = Vec<Span<'a>>;
    fn next(&mut self) -> Option<Vec<Span<'a>>> {
        let line = self.lines.get(self.i)?;
        let next = self.lines.get(self.i + 1);
        let has_comma = match next {
            None => false,
            Some(line) => line.is_closing(),
        };
        let indent_span = Span::raw("  ".repeat(self.indent));
        let mut out = match &line.key {
            Some(key) if !line.is_closing() => vec![
                indent_span,
                Span::raw(format!("{:?}", key)),
                Span::raw(" : "),
            ],
            _ => vec![indent_span],
        };
        let style = if self.i == self.cursor {
            Style::default().bg(Color::Blue)
        } else {
            Style::default()
        };
        match line {
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
                self.indent += 1;
            }
            Line {
                content: LineContent::ArrayEnd(_),
                folded: false,
                ..
            } => {
                out.push(Span::styled("]", style));
                self.indent -= 1;
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
                self.indent += 1;
            }
            Line {
                content: LineContent::ObjectEnd(_),
                folded: false,
                ..
            } => {
                out.push(Span::styled("}", style));
                self.indent -= 1;
            }
        };
        self.i += line.next_displayed_offset();
        Some(out)
    }
}
