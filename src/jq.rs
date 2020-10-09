use jq_rs;
use jq_sys::{
    jv, jv_array, jv_bool, jv_copy, jv_free, jv_null, jv_number, jv_object, jv_string_sized,
};
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

pub struct JV {
    ptr: jv,
}

impl Drop for JV {
    fn drop(&mut self) {
        unsafe { jv_free(self.ptr) }
    }
}

impl Clone for JV {
    fn clone(&self) -> Self {
        JV {
            ptr: unsafe { jv_copy(self.ptr) },
        }
    }
}

impl JV {
    pub fn array() -> Self {
        JV {
            ptr: unsafe { jv_array() },
        }
    }
    pub fn object() -> Self {
        JV {
            ptr: unsafe { jv_object() },
        }
    }
    pub fn bool(b: bool) -> Self {
        JV {
            ptr: unsafe { jv_bool(b.into()) },
        }
    }
    pub fn number(f: f64) -> Self {
        JV {
            ptr: unsafe { jv_number(f) },
        }
    }
    pub fn string(s: &str) -> Self {
        JV {
            ptr: unsafe { jv_string_sized(s) },
        }
    }
    pub fn null() -> Self {
        JV {
            ptr: unsafe { jv_null() },
        }
    }
}
