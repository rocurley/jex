pub mod jv;

use jq_sys::{jq_compile, jq_init, jq_next, jq_set_error_cb, jq_start, jq_state, jq_teardown};
use jv::{JVKind, JVRaw, JV};
use std::{convert::TryInto, ffi::CString, os::raw::c_void};

pub fn run_jq_query<'a, I: IntoIterator<Item = &'a JV>>(
    content: I,
    prog: &mut JQ,
) -> Result<Vec<JV>, String> {
    let mut results: Vec<JV> = Vec::new();
    for value in content {
        for res in prog.execute(value.clone().into()) {
            results.push(res.try_into()?);
        }
    }
    Ok(results)
}

#[derive(Debug)]
pub struct JQ {
    ptr: *mut jq_state,
    // We want to make sure the vec pointer doesn't move, so we can keep pushing to it.
    #[allow(clippy::box_vec)]
    errors: Box<Vec<JVRaw>>,
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
        let err_ptr = (errors.as_mut() as *mut Vec<JVRaw>) as *mut c_void;
        unsafe { jq_set_error_cb(ptr, Some(jq_error_callback), err_ptr) };
        JQ { ptr, errors }
    }
    fn take_errors(&mut self) -> impl Iterator<Item = JVRaw> + '_ {
        self.errors.as_mut().drain(..)
    }
    pub fn compile(s: &str) -> Result<Self, Vec<String>> {
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
            Err(strings)
        }
    }
    fn execute(&mut self, input: JVRaw) -> impl Iterator<Item = JVRaw> + '_ {
        let raw: JVRaw = input.into();
        unsafe { jq_start(self.ptr, raw.unwrap_without_drop(), 0) };
        JQResults { jq: self }
    }
}

unsafe extern "C" fn jq_error_callback(data_pointer: *mut c_void, data: jq_sys::jv) {
    let casted_pointer = data_pointer as *mut Vec<JVRaw>;
    if let Some(errors) = casted_pointer.as_mut() {
        errors.push(JVRaw { ptr: data });
    }
}

struct JQResults<'a> {
    jq: &'a mut JQ,
}

impl<'a> Iterator for JQResults<'a> {
    type Item = JVRaw;
    fn next(&mut self) -> Option<Self::Item> {
        let res = JVRaw {
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
    use super::{run_jq_query, JVRaw, JQ};
    use crate::{jq::jv::JV, testing::arb_json};
    use proptest::proptest;
    use serde_json::{json, value::Value};
    use std::cell::RefCell;
    fn sample_json() -> JV {
        let val = json!({
            "hello": "world",
            "array": ["a", "b", "c", 1.0, 2.0, 3.0],
        });
        (&val).into()
    }
    #[test]
    fn prop_jq_roundtrip() {
        let jq = JQ::compile(".").unwrap();
        let jq_cell = RefCell::new(jq);
        proptest!(move |(value in arb_json())| {
            let jv = JVRaw::from_serde(&value);
            let mut jq = jq_cell.borrow_mut();
            let results : Vec<Value> = jq.execute(jv).map(|jv| jv.to_serde().unwrap()).collect();
            assert_eq!(vec![value], results);
        })
    }
    #[test]
    fn unit_jq_simple() {
        let mut prog = JQ::compile(".array").unwrap();
        let res = run_jq_query(&[sample_json()], &mut prog).unwrap();
        assert_eq!(res, vec![(&json!(["a", "b", "c", 1.0, 2.0, 3.0])).into()]);
    }
    #[test]
    fn unit_jq_spread() {
        let mut prog = JQ::compile(".array | .[]").unwrap();
        let res = run_jq_query(&[sample_json()], &mut prog).unwrap();
        assert_eq!(
            res,
            vec![
                (&json!("a")).into(),
                (&json!("b")).into(),
                (&json!("c")).into(),
                (&json!(1.0)).into(),
                (&json!(2.0)).into(),
                (&json!(3.0)).into()
            ]
        );
    }
    #[test]
    fn unit_jq_invalid_program() {
        let prog = JQ::compile("lol");
        assert!(prog.is_err());
        let expected = vec![
            "jq: error: lol/0 is not defined at <top-level>, line 1:\nlol",
            "jq: 1 compile error",
        ];
        assert_eq!(prog.unwrap_err(), expected);
    }
    #[test]
    fn unit_jq_runtime_error() {
        let mut prog = JQ::compile(".[1]").unwrap();
        let res = run_jq_query(&[sample_json()], &mut prog);
        assert_eq!(res.unwrap_err(), "Cannot index object with number");
    }
}
