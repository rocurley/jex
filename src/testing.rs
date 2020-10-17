use crate::lines::{Line, LineContent};
use proptest::prelude::*;
use serde_json::value::Value;
pub fn arb_json() -> impl Strategy<Value = Value> {
    let leaf = prop_oneof![
        Just(Value::Null),
        any::<bool>().prop_map(Value::Bool),
        any::<f64>().prop_map(|f| f.into()),
        ".*".prop_map(Value::String),
    ];
    leaf.prop_recursive(
        8,   // 8 levels deep
        256, // Shoot for maximum size of 256 nodes
        10,  // We put up to 10 items per collection
        |inner| {
            prop_oneof![
                // Take the inner strategy and make the two recursive cases.
                prop::collection::vec(inner.clone(), 0..10).prop_map(Value::Array),
                prop::collection::hash_map(".*", inner, 0..10)
                    .prop_map(|m| { Value::Object(m.into_iter().collect()) }),
            ]
        },
    )
}

pub fn json_to_lines<'a, I: Iterator<Item = &'a Value>>(vs: I) -> Vec<Line> {
    let mut out = Vec::new();
    for value in vs {
        json_to_lines_inner(None, value, 0, &mut out, false);
    }
    out
}

fn push_line(
    key: Option<Box<str>>,
    content: LineContent,
    indent: u8,
    out: &mut Vec<Line>,
    comma: bool,
) {
    let line = Line {
        content,
        key,
        folded: false,
        indent,
        comma,
    };
    out.push(line);
}

fn json_to_lines_inner(
    key: Option<Box<str>>,
    v: &Value,
    indent: u8,
    out: &mut Vec<Line>,
    comma: bool,
) -> usize {
    match v {
        Value::Null => {
            push_line(key, LineContent::Null, indent, out, comma);
            1
        }
        Value::Bool(b) => {
            push_line(key, LineContent::Bool(*b), indent, out, comma);
            1
        }
        Value::Number(x) => {
            push_line(key, LineContent::Number(x.clone()), indent, out, comma);
            1
        }
        Value::String(s) => {
            push_line(
                key,
                LineContent::String(s.as_str().into()),
                indent,
                out,
                comma,
            );
            1
        }
        Value::Array(xs) => {
            let mut count = 0;
            let start_position = out.len();
            push_line(key, LineContent::ArrayStart(0), indent, out, false);
            for (i, x) in xs.iter().enumerate() {
                let comma = i != xs.len() - 1;
                count += json_to_lines_inner(None, x, indent + 1, out, comma);
            }
            push_line(None, LineContent::ArrayEnd(count), indent, out, comma);
            out[start_position].content = LineContent::ArrayStart(count);
            count + 2
        }
        Value::Object(xs) => {
            let mut count = 0;
            let start_position = out.len();
            push_line(key, LineContent::ObjectStart(0), indent, out, false);
            for (i, (k, x)) in xs.iter().enumerate() {
                let comma = i != xs.len() - 1;
                count += json_to_lines_inner(Some(k.as_str().into()), x, indent + 1, out, comma);
            }
            push_line(None, LineContent::ObjectEnd(count), indent, out, comma);
            out[start_position].content = LineContent::ObjectStart(count);
            count + 2
        }
    }
}
