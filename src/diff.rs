use crate::jq::jv::{JVBool, JVNull, JVNumber, JVString, JV};
use similar::{capture_diff, Algorithm, DiffOp};

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum DiffElem {
    Null(JVNull),
    Bool(JVBool),
    Number(JVNumber),
    String(JVString),
    ObjectStart,
    ObjectEnd,
    ArrayStart,
    ArrayEnd,
}

fn to_diffable(jv: JV) -> Vec<DiffElem> {
    let mut out = Vec::new();
    write_diffable(jv, &mut out);
    out
}

fn write_diffable(jv: JV, out: &mut Vec<DiffElem>) {
    match jv {
        JV::Null(x) => out.push(DiffElem::Null(x)),
        JV::Bool(x) => out.push(DiffElem::Bool(x)),
        JV::Number(x) => out.push(DiffElem::Number(x)),
        JV::String(x) => out.push(DiffElem::String(x)),
        JV::Object(obj) => {
            out.push(DiffElem::ObjectStart);
            let mut kvs: Vec<(JVString, JV)> = obj.into_iter().map(|(k, v)| (k, v)).collect();
            kvs.sort_by(|x, y| x.0.cmp(&y.0));
            for (k, v) in kvs {
                write_diffable(k.into(), out);
                write_diffable(v, out);
            }
            out.push(DiffElem::ObjectEnd);
        }
        JV::Array(arr) => {
            out.push(DiffElem::ArrayStart);
            for child in arr.iter() {
                write_diffable(child, out);
            }
            out.push(DiffElem::ArrayEnd);
        }
    }
}

fn diff(a: JV, b: JV) -> Vec<DiffOp> {
    let diffable_a = to_diffable(a);
    let diffable_b = to_diffable(b);
    capture_diff(
        Algorithm::Patience,
        &diffable_a,
        0..diffable_a.len(),
        &diffable_b,
        0..diffable_b.len(),
    )
}

#[cfg(test)]
mod tests {
    use super::{diff, to_diffable};
    use crate::jq::jv::JV;
    use serde_json::json;
    #[test]
    fn unit_diff() {
        let a: JV = (&json!({
            "A" : {"Hello":"World"},
            "B" : {"Foo":"Bar"},
        }))
            .into();
        let b: JV = (&json!({
            "B" : {"Hello":"World"},
            "A" : {"Foo":"Bar"},
        }))
            .into();
        dbg!(to_diffable(a.clone()));
        dbg!(to_diffable(b.clone()));
        dbg!(diff(a, b));
    }
}
