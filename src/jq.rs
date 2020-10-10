use jq_rs;
use jq_sys::{
    jv, jv_array, jv_array_get, jv_array_length, jv_array_set, jv_bool, jv_copy, jv_free,
    jv_get_kind, jv_kind, jv_kind_JV_KIND_ARRAY, jv_kind_JV_KIND_FALSE, jv_kind_JV_KIND_INVALID,
    jv_kind_JV_KIND_NULL, jv_kind_JV_KIND_NUMBER, jv_kind_JV_KIND_OBJECT, jv_kind_JV_KIND_STRING,
    jv_kind_JV_KIND_TRUE, jv_null, jv_number, jv_number_value, jv_object, jv_object_iter,
    jv_object_iter_key, jv_object_iter_next, jv_object_iter_valid, jv_object_iter_value,
    jv_object_set, jv_string_sized, jv_string_value,
};
use serde_json::{value::Value, Deserializer, Number};
use std::{convert::TryInto, ffi::CStr, iter::FromIterator, mem::forget, os::raw::c_char};
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
    pub fn empty_array() -> Self {
        JV {
            ptr: unsafe { jv_array() },
        }
    }
    pub fn array_set(&mut self, i: i32, x: JV) {
        self.ptr = unsafe { jv_array_set(self.ptr, i, x.ptr) };
        forget(x);
    }
    pub fn empty_object() -> Self {
        JV {
            ptr: unsafe { jv_object() },
        }
    }
    pub fn object_set(&mut self, k: &str, v: JV) {
        let key = JV::string(k);
        self.ptr = unsafe { jv_object_set(self.ptr, key.ptr, v.ptr) };
        forget(v);
        forget(key);
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
    pub fn from_serde(v: &Value) -> Self {
        match v {
            Value::Null => JV::null(),
            Value::Bool(b) => JV::bool(*b),
            Value::Number(n) => JV::number(n.as_f64().expect("Non-f64 number")),
            Value::String(s) => JV::string(s),
            Value::Array(xs) => xs.iter().map(JV::from_serde).collect(),
            Value::Object(obj) => obj
                .iter()
                .map(|(k, v)| (k.as_str(), JV::from_serde(v)))
                .collect(),
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
    pub fn number_value(&self) -> f64 {
        unsafe { jv_number_value(self.ptr) }
    }
    pub fn string_value(&self) -> &str {
        let c_str = unsafe {
            let string_ptr = jv_string_value(self.ptr);
            CStr::from_ptr(string_ptr)
        };
        c_str.to_str().expect("JQ strings are supposed to be UTF-8")
    }
    pub fn object_iter<'a>(&'a self) -> impl Iterator<Item = (String, JV)> + 'a {
        let i = unsafe { jv_object_iter(self.ptr) };
        ObjectIterator { i, obj: self }
    }
    pub fn array_iter<'a>(&'a self) -> impl Iterator<Item = JV> + 'a {
        (0..unsafe { jv_array_length(self.clone().ptr) })
            .into_iter()
            .map(move |i| JV {
                ptr: unsafe { jv_array_get(self.clone().ptr, i) },
            })
    }
    pub fn to_serde(&self) -> Option<Value> {
        match self.get_kind() {
            JVKind::Invalid => None,
            JVKind::Null => Some(Value::Null),
            JVKind::False => Some(Value::Bool(false)),
            JVKind::True => Some(Value::Bool(true)),
            JVKind::Number => Some(self.number_value().into()),
            JVKind::String => Some(self.string_value().into()),
            JVKind::Array => Some(
                self.array_iter()
                    .map(|x| x.to_serde().expect("Array element should not be invalid"))
                    .collect(),
            ),
            JVKind::Object => Some(Value::Object(
                self.object_iter()
                    .map(|(k, v)| {
                        (
                            k,
                            v.to_serde().expect("Object element should not be invalid"),
                        )
                    })
                    .collect(),
            )),
        }
    }
}

impl FromIterator<JV> for JV {
    fn from_iter<I: IntoIterator<Item = JV>>(iter: I) -> Self {
        let mut out = JV::empty_array();
        for (i, x) in iter.into_iter().enumerate() {
            out.array_set(i as i32, x);
        }
        out
    }
}

impl<'a> FromIterator<(&'a str, JV)> for JV {
    fn from_iter<I: IntoIterator<Item = (&'a str, JV)>>(iter: I) -> Self {
        let mut out = JV::empty_object();
        for (k, v) in iter.into_iter() {
            out.object_set(k, v);
        }
        out
    }
}

struct ObjectIterator<'a> {
    i: i32,
    obj: &'a JV,
}

impl<'a> Iterator for ObjectIterator<'a> {
    type Item = (String, JV);
    fn next(&mut self) -> Option<Self::Item> {
        if unsafe { jv_object_iter_valid(self.obj.ptr, self.i) } == 0 {
            return None;
        }
        let k = JV {
            ptr: unsafe { jv_object_iter_key(self.obj.ptr, self.i) },
        };
        let v = JV {
            ptr: unsafe { jv_object_iter_value(self.obj.ptr, self.i) },
        };
        // If we wanted to live dangerously, we could say something like this:
        // Because jv values are COW, k's string value will stay valid as long as obj lives,
        // so we can return a &'a str. That's too spooky for now though.
        self.i = unsafe { jv_object_iter_next(self.obj.ptr, self.i) };
        Some((k.string_value().into(), v))
    }
}

#[cfg(test)]
mod tests {}
