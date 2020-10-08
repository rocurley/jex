use serde_json::value::{Number, Value};

#[derive(Debug, Clone)]
pub struct Line {
    pub content: LineContent,
    pub key: Option<String>,
    pub folded: bool,
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
    End,
}

pub fn jsons_to_lines<'a, I: Iterator<Item = &'a Value>>(vs: I) -> Vec<Line> {
    let mut out = Vec::new();
    for value in vs {
        json_to_lines_inner(None, value, &mut out);
    }
    out
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
            count
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
            count
        }
    }
}
