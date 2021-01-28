use crate::{
    jq::jv::JVString,
    lines::{Leaf, LeafContent},
};
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

pub fn json_to_lines<'a, I: Iterator<Item = &'a Value>>(vs: I) -> Vec<Leaf> {
    let mut out = Vec::new();
    for value in vs {
        json_to_lines_inner(None, value, 0, &mut out, false);
    }
    out
}

fn push_line(
    key: Option<JVString>,
    content: LeafContent,
    indent: u16,
    out: &mut Vec<Leaf>,
    comma: bool,
) {
    let line = Leaf {
        content,
        key,
        indent,
        comma,
    };
    out.push(line);
}

fn json_to_lines_inner(
    key: Option<JVString>,
    v: &Value,
    indent: u16,
    out: &mut Vec<Leaf>,
    comma: bool,
) {
    match v {
        Value::Null => {
            push_line(key, LeafContent::Null, indent, out, comma);
        }
        Value::Bool(b) => {
            push_line(key, LeafContent::Bool(*b), indent, out, comma);
        }
        Value::Number(x) => {
            push_line(
                key,
                LeafContent::Number(x.as_f64().unwrap()),
                indent,
                out,
                comma,
            );
        }
        Value::String(s) => {
            push_line(
                key,
                LeafContent::String(JVString::new(s)),
                indent,
                out,
                comma,
            );
        }
        Value::Array(xs) => {
            push_line(key, LeafContent::ArrayStart, indent, out, false);
            for (i, x) in xs.iter().enumerate() {
                let comma = i != xs.len() - 1;
                json_to_lines_inner(None, x, indent + 2, out, comma);
            }
            push_line(None, LeafContent::ArrayEnd, indent, out, comma);
        }
        Value::Object(xs) => {
            push_line(key, LeafContent::ObjectStart, indent, out, false);
            for (i, (k, x)) in xs.iter().enumerate() {
                let comma = i != xs.len() - 1;
                json_to_lines_inner(Some(JVString::new(k)), x, indent + 2, out, comma);
            }
            push_line(None, LeafContent::ObjectEnd, indent, out, comma);
        }
    }
}
