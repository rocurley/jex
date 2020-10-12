use serde_json::value::{Number, Value};
use tui::{
    style::{Color, Modifier, Style},
    text::{Span, Spans},
};

#[derive(Debug, Clone)]
pub struct Line {
    pub content: LineContent,
    pub key: Option<String>,
    pub folded: bool,
    pub indent: usize,
}

impl Line {
    fn is_closing(&self) -> bool {
        match self.content {
            LineContent::ArrayEnd(_) => true,
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
    vs.map(|value| {
        let mut out = Vec::new();
        json_to_lines_inner(None, value, 0, &mut out);
        out
    })
    .collect()
}

fn push_line(key: Option<String>, content: LineContent, indent: usize, out: &mut Vec<Line>) {
    let line = Line {
        content,
        key,
        folded: false,
        indent,
    };
    out.push(line);
}

fn json_to_lines_inner(
    key: Option<String>,
    v: &Value,
    indent: usize,
    out: &mut Vec<Line>,
) -> usize {
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
            push_line(key, LineContent::String(s.clone()), indent, out);
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
                count += json_to_lines_inner(Some(k.clone()), x, indent + 1, out);
            }
            push_line(None, LineContent::ObjectEnd(count), indent, out);
            out[start_position].content = LineContent::ObjectStart(count);
            count + 2
        }
    }
}

pub fn render_lines<'a>(
    mut scroll: usize,
    mut line_limit: u16,
    cursor: &'a Option<(usize, usize)>,
    lines: &'a [Vec<Line>],
) -> Vec<Spans<'a>> {
    let mut out = Vec::with_capacity(line_limit.into());
    for (i, value_lines) in lines.iter().enumerate() {
        if value_lines.len() <= scroll {
            scroll -= value_lines.len();
            continue;
        }
        let cursor = cursor.and_then(
            |(value_ix, line_ix)| {
                if value_ix == i {
                    Some(line_ix)
                } else {
                    None
                }
            },
        );
        let value_lines = JsonText {
            lines: value_lines,
            cursor,
            i: scroll,
        };
        for line in value_lines.take(line_limit.into()) {
            out.push(Spans::from(line));
            line_limit -= 1;
        }
        scroll = 0;
        if line_limit == 0 {
            return out;
        }
    }
    out
}

struct JsonText<'a> {
    lines: &'a [Line],
    cursor: Option<usize>,
    i: usize,
}
impl<'a> Iterator for JsonText<'a> {
    type Item = Vec<Span<'a>>;
    fn next(&mut self) -> Option<Vec<Span<'a>>> {
        let line = self.lines.get(self.i)?;
        let next = self.lines.get(self.i + 1);
        let has_comma = match next {
            None => false,
            Some(line) => !line.is_closing(),
        };
        let indent_span = Span::raw("  ".repeat(line.indent));
        let mut out = match &line.key {
            Some(key) => vec![
                indent_span,
                Span::raw(format!("{:?}", key)),
                Span::raw(" : "),
            ],
            _ => vec![indent_span],
        };
        let style = if Some(self.i) == self.cursor {
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
        self.i += line.next_displayed_offset();
        Some(out)
    }
}
