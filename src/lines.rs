use serde_json::value::Number;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    pub content: LineContent,
    pub key: Option<Box<str>>,
    pub folded: bool,
    pub indent: u8,
    pub comma: bool,
}

pub fn next_displayable_line_raw(i: usize, line: &Line) -> usize {
    let delta = match line {
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
    i + delta
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LineContent {
    Null,
    Bool(bool),
    Number(Number),
    String(Box<str>),
    ArrayStart(usize),
    ArrayEnd(usize),
    ObjectStart(usize),
    ObjectEnd(usize),
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
                ValueTerminator => {
                    self.value_terminator += MemoryStat {
                        count: 1,
                        json_size: 1, // Newlines, including trailing newline
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
