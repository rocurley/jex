use jq_rs;
use serde_json::{value::Value, Deserializer};
pub fn run_jq_query(content: &[Value], prog: &mut jq_rs::JqProgram) -> Vec<Value> {
    let right_strings: Vec<String> = content
        .iter()
        .map(|j| prog.run(&j.to_string()).expect("jq execution error"))
        .collect();
    let right_content: Result<Vec<Value>, _> = right_strings
        .iter()
        .flat_map(|j| Deserializer::from_str(j).into_iter::<Value>())
        .collect();
    right_content.expect("json decoding error")
}
