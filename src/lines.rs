use tui::{
    style::{Color, Modifier, Style},
    text::{Span, Spans},
};

#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub content: LineContent,
    pub key: Option<Box<str>>,
    pub indent: u8,
    pub comma: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum LineContent {
    Null,
    Bool(bool),
    Number(f64),
    String(Box<str>),
    FoldedArray(usize),
    ArrayStart,
    ArrayEnd,
    FoldedObject(usize),
    ObjectStart,
    ObjectEnd,
}

impl Line {
    pub fn render(self, is_cursor: bool) -> Spans<'static> {
        let indent_span = Span::raw("  ".repeat(self.indent as usize));
        let mut out = match &self.key {
            Some(key) => vec![
                indent_span,
                Span::raw(format!("{:?}", key)),
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
                out.push(Span::styled(format!("{:?}", s), style));
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
