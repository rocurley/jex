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
    String(String),
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
    let indent_span = Span::raw("  ".repeat(line.indent));
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

struct JsonText<'a> {
    lines: &'a [Line],
    cursor: Option<usize>,
    i: Option<usize>,
}
impl<'a> Iterator for JsonText<'a> {
    type Item = Vec<Span<'a>>;
    fn next(&mut self) -> Option<Vec<Span<'a>>> {
        let i = self.i?;
        let line = &self.lines[i];
        let next = self.lines.get(i + 1);
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
        let style = if Some(i) == self.cursor {
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
        self.i = next_displayable_line(i, &self.lines);
        Some(out)
    }
}
