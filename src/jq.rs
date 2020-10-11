pub mod jv;

use jq_sys::{
    jq_compile, jq_format_error, jq_init, jq_next, jq_set_error_cb, jq_start, jq_state, jq_teardown,
};
use jv::{JVKind, JV};
use serde_json::value::Value;
use std::{ffi::CString, os::raw::c_void};

pub fn run_jq_query<'a, I: IntoIterator<Item = &'a Value>>(
    content: I,
    prog: &mut JQ,
) -> Result<Vec<Value>, String> {
    let mut results: Vec<Value> = Vec::new();
    for value in content {
        let jv = JV::from_serde(value);
        for res in prog.execute(jv) {
            results.push(res.to_serde()?);
        }
    }
    Ok(results)
}

#[derive(Debug)]
pub struct JQ {
    ptr: *mut jq_state,
    // We want to make sure the vec pointer doesn't move, so we can keep pushing to it.
    #[allow(clippy::box_vec)]
    errors: Box<Vec<JV>>,
}

impl Drop for JQ {
    fn drop(&mut self) {
        unsafe { jq_teardown(&mut self.ptr) };
    }
}

impl JQ {
    fn new() -> Self {
        let ptr = unsafe { jq_init() };
        let mut errors = Box::new(Vec::new());
        let err_ptr = (errors.as_mut() as *mut Vec<JV>) as *mut c_void;
        unsafe { jq_set_error_cb(ptr, Some(jq_error_callback), err_ptr) };
        JQ { ptr, errors }
    }
    pub fn take_errors(&mut self) -> impl Iterator<Item = JV> + '_ {
        self.errors.as_mut().drain(..)
    }
    pub fn compile(s: &str) -> Result<Self, String> {
        let mut prog = JQ::new();
        let cstr = CString::new(s).expect("Nul byte in jq program");
        let ok = unsafe { jq_compile(prog.ptr, cstr.as_ptr()) };
        if ok > 0 {
            Ok(prog)
        } else {
            let strings: Vec<String> = prog
                .take_errors()
                .map(|jv| jv.string_value().to_owned())
                .collect();
            Err(strings.join("\n"))
        }
    }
    pub fn execute(&mut self, input: JV) -> impl Iterator<Item = JV> + '_ {
        unsafe { jq_start(self.ptr, input.unwrap_without_drop(), 0) };
        JQResults { jq: self }
    }
}

unsafe extern "C" fn jq_error_callback(data_pointer: *mut c_void, data: jq_sys::jv) {
    let casted_pointer = data_pointer as *mut Vec<JV>;
    if let Some(errors) = casted_pointer.as_mut() {
        errors.push(JV { ptr: data });
    }
}

pub fn format_jq_error(jv: JV) -> JV {
    JV {
        ptr: unsafe { jq_format_error(jv.unwrap_without_drop()) },
    }
}

struct JQResults<'a> {
    jq: &'a mut JQ,
}

impl<'a> Iterator for JQResults<'a> {
    type Item = JV;
    fn next(&mut self) -> Option<Self::Item> {
        let res = JV {
            ptr: unsafe { jq_next(self.jq.ptr) },
        };
        match res.get_kind() {
            JVKind::Invalid => {
                if res.invalid_has_msg() {
                    Some(res)
                } else {
                    None
                }
            }
            _ => Some(res),
        }
    }
}

impl<'a> Drop for JQResults<'a> {
    fn drop(&mut self) {
        // Clear the error callback so we never attempt to modify error after it's freed.  drop is
        // not guaranteed to be called, but if it isn't called, then error won't be freed anyway,
        // so that's not an issue.
        unsafe { jq_set_error_cb(self.jq.ptr, None, std::ptr::null_mut()) };
    }
}

#[cfg(test)]
mod tests {
    use super::{run_jq_query, JQ, JV};
    use crate::testing::arb_json;
    use proptest::proptest;
    use serde_json::{json, value::Value};
    use std::cell::RefCell;
    fn sample_json() -> Value {
        json!({
            "hello": "world",
            "array": ["a", "b", "c", 1.0, 2.0, 3.0],
        })
    }
    #[test]
    fn prop_jq_roundtrip() {
        let jq = JQ::compile(".").unwrap();
        let jq_cell = RefCell::new(jq);
        proptest!(move |(value in arb_json())| {
            let jv = JV::from_serde(&value);
            let mut jq = jq_cell.borrow_mut();
            let results : Vec<Value> = jq.execute(jv).map(|jv| jv.to_serde().unwrap()).collect();
            assert_eq!(vec![value], results);
        })
    }
    #[test]
    fn unit_jq_simple() {
        let mut prog = JQ::compile(".array").unwrap();
        let res = run_jq_query(&[sample_json()], &mut prog).unwrap();
        assert_eq!(res, vec![json!(["a", "b", "c", 1.0, 2.0, 3.0])]);
    }
    #[test]
    fn unit_jq_spread() {
        let mut prog = JQ::compile(".array | .[]").unwrap();
        let res = run_jq_query(&[sample_json()], &mut prog).unwrap();
        assert_eq!(
            res,
            vec![
                json!("a"),
                json!("b"),
                json!("c"),
                json!(1.0),
                json!(2.0),
                json!(3.0)
            ]
        );
    }
    #[test]
    fn unit_jq_invalid_program() {
        let prog = JQ::compile("lol");
        assert!(prog.is_err());
        let expected =
            "jq: error: lol/0 is not defined at <top-level>, line 1:\nlol\njq: 1 compile error";
        assert_eq!(prog.unwrap_err(), expected);
    }
    #[test]
    fn unit_jq_runtime_error() {
        let mut prog = JQ::compile(".[1]").unwrap();
        let res = run_jq_query(&[sample_json()], &mut prog);
        let errors: Vec<_> = prog.take_errors().collect();
        assert_eq!(res.unwrap_err(), "Cannot index object with number");
    }
}
