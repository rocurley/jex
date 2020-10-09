use jq_rs;
use jq_sys::{
    jv, jv_array, jv_bool, jv_copy, jv_free, jv_get_kind, jv_kind, jv_kind_JV_KIND_ARRAY,
    jv_kind_JV_KIND_FALSE, jv_kind_JV_KIND_INVALID, jv_kind_JV_KIND_NULL, jv_kind_JV_KIND_NUMBER,
    jv_kind_JV_KIND_OBJECT, jv_kind_JV_KIND_STRING, jv_kind_JV_KIND_TRUE, jv_null, jv_number,
    jv_object, jv_string_sized,
};
use serde_json::{value::Value, Deserializer};
use std::{convert::TryInto, os::raw::c_char};
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

pub enum JVKind {
    Invalid = jv_kind_JV_KIND_INVALID as isize,
    Null = jv_kind_JV_KIND_NULL as isize,
    False = jv_kind_JV_KIND_FALSE as isize,
    True = jv_kind_JV_KIND_TRUE as isize,
    Number = jv_kind_JV_KIND_NUMBER as isize,
    String = jv_kind_JV_KIND_STRING as isize,
    Array = jv_kind_JV_KIND_ARRAY as isize,
    Object = jv_kind_JV_KIND_OBJECT as isize,
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
        // JV makes a copy of the string in jv_string_sized, which is then owned by the jv value.
        JV {
            ptr: unsafe {
                jv_string_sized(s.as_ptr() as *const c_char, s.len().try_into().unwrap())
            },
        }
    }
    pub fn null() -> Self {
        JV {
            ptr: unsafe { jv_null() },
        }
    }
    pub fn get_kind(&self) -> JVKind {
        let raw_kind = unsafe { jv_get_kind(self.ptr) };
        match raw_kind {
            jv_kind_JV_KIND_INVALID => JVKind::Invalid,
            jv_kind_JV_KIND_NULL => JVKind::Null,
            jv_kind_JV_KIND_FALSE => JVKind::False,
            jv_kind_JV_KIND_TRUE => JVKind::True,
            jv_kind_JV_KIND_NUMBER => JVKind::Number,
            jv_kind_JV_KIND_STRING => JVKind::String,
            jv_kind_JV_KIND_ARRAY => JVKind::Array,
            jv_kind_JV_KIND_OBJECT => JVKind::Object,
            _ => panic!("Invalid kind"),
        }
    }
    pub fn to_serde(self) -> Option<Value> {
        match self.get_kind() {
            JVKind::Invalid => None,
            JVKind::Null => Some(Value::Null),
            JVKind::False => Some(Value::Bool(false)),
            JVKind::True => Some(Value::Bool(true)),
            JVKind::Number => unimplemented!(),
            JVKind::String => unimplemented!(),
            JVKind::Array => unimplemented!(),
            JVKind::Object => unimplemented!(),
        }
    }
}
