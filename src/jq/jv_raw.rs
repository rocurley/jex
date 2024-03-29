use super::jv::{JVString, JV};
use jq_sys::{
    jv, jv_array, jv_array_get, jv_array_length, jv_array_set, jv_bool, jv_copy, jv_equal, jv_free,
    jv_get_kind, jv_get_refcnt, jv_invalid_get_msg, jv_invalid_has_msg, jv_kind_JV_KIND_ARRAY,
    jv_kind_JV_KIND_FALSE, jv_kind_JV_KIND_INVALID, jv_kind_JV_KIND_NULL, jv_kind_JV_KIND_NUMBER,
    jv_kind_JV_KIND_OBJECT, jv_kind_JV_KIND_STRING, jv_kind_JV_KIND_TRUE, jv_null, jv_number,
    jv_number_value, jv_object, jv_object_get, jv_object_iter, jv_object_iter_key,
    jv_object_iter_next, jv_object_iter_valid, jv_object_iter_value, jv_object_length,
    jv_object_set, jv_parse_sized, jv_string_length_bytes, jv_string_sized, jv_string_value,
};
use serde_json::value::Value;
use std::{
    convert::{TryFrom, TryInto},
    fmt,
    hash::{Hash, Hasher},
    iter::FromIterator,
    mem::forget,
    os::raw::c_char,
    slice, str,
};

#[repr(transparent)]
pub struct JVRaw {
    pub ptr: jv,
}
impl fmt::Debug for JVRaw {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "JV{{..}}")
    }
}

impl Drop for JVRaw {
    fn drop(&mut self) {
        unsafe { jv_free(self.ptr) }
    }
}

impl Clone for JVRaw {
    fn clone(&self) -> Self {
        JVRaw {
            ptr: unsafe { jv_copy(self.ptr) },
        }
    }
}

impl PartialEq for JVRaw {
    fn eq(&self, other: &Self) -> bool {
        let self_ptr = self.clone().unwrap_without_drop();
        let other_ptr = other.clone().unwrap_without_drop();
        let res = unsafe { jv_equal(self_ptr, other_ptr) };
        res != 0
    }
}
impl Eq for JVRaw {}

#[derive(Clone, Copy, Eq, Debug, PartialEq)]
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

impl JVRaw {
    pub fn unwrap_without_drop(self) -> jv {
        let JVRaw { ptr } = self;
        forget(self);
        ptr
    }
    pub fn empty_array() -> Self {
        JVRaw {
            ptr: unsafe { jv_array() },
        }
    }
    pub fn array_set(&mut self, i: i32, x: JVRaw) {
        self.ptr = unsafe { jv_array_set(self.ptr, i, x.unwrap_without_drop()) };
    }
    pub fn empty_object() -> Self {
        JVRaw {
            ptr: unsafe { jv_object() },
        }
    }
    pub fn object_set(&mut self, k: &str, v: JVRaw) {
        let key = JVRaw::string(k);
        self.ptr =
            unsafe { jv_object_set(self.ptr, key.unwrap_without_drop(), v.unwrap_without_drop()) };
    }
    pub fn object_get(&self, k: &str) -> JVRaw {
        let key = JVRaw::string(k);
        let ptr = unsafe {
            jv_object_get(
                self.clone().unwrap_without_drop(),
                key.unwrap_without_drop(),
            )
        };
        JVRaw { ptr }
    }
    pub fn bool(b: bool) -> Self {
        JVRaw {
            ptr: unsafe { jv_bool(b.into()) },
        }
    }
    pub fn number(f: f64) -> Self {
        JVRaw {
            ptr: unsafe { jv_number(f) },
        }
    }
    pub fn string(s: &str) -> Self {
        // JV makes a copy of the string in jv_string_sized, which is then owned by the jv value.
        JVRaw {
            ptr: unsafe {
                jv_string_sized(s.as_ptr() as *const c_char, s.len().try_into().unwrap())
            },
        }
    }
    pub fn null() -> Self {
        JVRaw {
            ptr: unsafe { jv_null() },
        }
    }
    // from_serde is attached to JVRaw so we don't have to strip off the outer layer when stuffing
    // things into arrays.
    pub fn from_serde(v: &Value) -> Self {
        match v {
            Value::Null => JVRaw::null(),
            Value::Bool(b) => JVRaw::bool(*b),
            Value::Number(n) => JVRaw::number(n.as_f64().expect("Non-f64 number")),
            Value::String(s) => JVRaw::string(s),
            Value::Array(xs) => xs.iter().map(JVRaw::from_serde).collect(),
            Value::Object(obj) => obj
                .iter()
                .map(|(k, v)| (k.as_str(), JVRaw::from_serde(v)))
                .collect(),
        }
    }

    pub fn get_kind(&self) -> JVKind {
        let raw_kind = unsafe { jv_get_kind(self.ptr) };
        #[allow(non_upper_case_globals)]
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
        unsafe {
            let string_ptr = jv_string_value(self.ptr) as *const u8;
            let len = jv_string_length_bytes(self.clone().unwrap_without_drop());
            let slice = slice::from_raw_parts(
                string_ptr,
                len.try_into().expect("length cannot be parsed as usize"),
            );
            // Safety: JQ guarantees that strings are utf8. Not checking here is extremely
            // important from a performance perspective: we regularly call string_value, and need
            // it to be a constant-time operation.
            str::from_utf8_unchecked(slice)
        }
    }
    pub fn object_len(&self) -> i32 {
        unsafe { jv_object_length(self.clone().unwrap_without_drop()) }
    }
    pub fn object_iter(&self) -> ObjectIterator<'_> {
        let i = unsafe { jv_object_iter(self.ptr) };
        ObjectIterator {
            remaining: self.object_len() as usize,
            i,
            obj: self,
        }
    }
    pub fn into_object_iter(self) -> OwnedObjectIterator {
        let i = unsafe { jv_object_iter(self.ptr) };
        OwnedObjectIterator {
            remaining: self.object_len() as usize,
            i,
            obj: self,
        }
    }
    pub fn into_empty_object_iter(self) -> OwnedObjectIterator {
        OwnedObjectIterator {
            remaining: 0,
            i: -2,
            obj: self,
        }
    }
    pub fn object_values(&self) -> ObjectValuesIterator {
        let i = unsafe { jv_object_iter(self.ptr) };
        ObjectValuesIterator {
            remaining: self.object_len() as usize,
            i,
            obj: self,
        }
    }
    pub fn array_len(&self) -> i32 {
        unsafe { jv_array_length(self.clone().unwrap_without_drop()) }
    }
    pub fn array_get<'a>(&'a self, i: i32) -> JVRaw {
        let ptr = unsafe { jv_array_get(self.clone().unwrap_without_drop(), i) };
        JVRaw { ptr }
    }
    pub fn invalid_has_msg(&self) -> bool {
        (unsafe { jv_invalid_has_msg(self.clone().unwrap_without_drop()) }) != 0
    }
    pub fn get_invalid_msg(self) -> Option<String> {
        if self.invalid_has_msg() {
            let jv_msg = JVRaw {
                ptr: unsafe { jv_invalid_get_msg(self.unwrap_without_drop()) },
            };
            Some(jv_msg.string_value().to_owned())
        } else {
            None
        }
    }
    pub fn parse_native(s: &str) -> Self {
        JVRaw {
            ptr: unsafe {
                jv_parse_sized(s.as_ptr() as *const c_char, s.len().try_into().unwrap())
            },
        }
    }
    pub fn refcount(&self) -> i32 {
        unsafe { jv_get_refcnt(self.ptr) }
    }
}

impl Hash for JVRaw {
    fn hash<H: Hasher>(&self, state: &mut H) {
        JV::try_from(self.clone()).hash(state)
    }
}

impl FromIterator<JVRaw> for JVRaw {
    fn from_iter<I: IntoIterator<Item = JVRaw>>(iter: I) -> Self {
        let mut out = JVRaw::empty_array();
        for (i, x) in iter.into_iter().enumerate() {
            out.array_set(i as i32, x);
        }
        out
    }
}

impl<'a> FromIterator<(&'a str, JVRaw)> for JVRaw {
    fn from_iter<I: IntoIterator<Item = (&'a str, JVRaw)>>(iter: I) -> Self {
        let mut out = JVRaw::empty_object();
        for (k, v) in iter.into_iter() {
            out.object_set(k, v);
        }
        out
    }
}

pub struct ObjectIterator<'a> {
    remaining: usize,
    i: i32,
    obj: &'a JVRaw,
}

impl<'a> Iterator for ObjectIterator<'a> {
    type Item = (&'a str, JV);
    fn next(&mut self) -> Option<Self::Item> {
        if unsafe { jv_object_iter_valid(self.obj.ptr, self.i) } == 0 {
            return None;
        }
        let k = JVRaw {
            ptr: unsafe { jv_object_iter_key(self.obj.ptr, self.i) },
        };
        let v = JVRaw {
            ptr: unsafe { jv_object_iter_value(self.obj.ptr, self.i) },
        };
        // If we wanted to live dangerously, we could say something like this:
        // Because jv values are COW, k's string value will stay valid as long as obj lives,
        // so we can return a &'a str. That's too spooky for now though.
        self.i = unsafe { jv_object_iter_next(self.obj.ptr, self.i) };
        self.remaining -= 1;
        Some((
            // Safety: While k will be dropped at the end of this function call, at least one copy
            // of it will remain as a part of obj as long as obj lives. As such, it's safe to cast
            // the lifetime to 'a.
            unsafe { std::mem::transmute(k.string_value()) },
            v.try_into().expect("Object should not contain invalid JV"),
        ))
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.remaining, Some(self.remaining))
    }
}

impl<'a> ExactSizeIterator for ObjectIterator<'a> {}

#[derive(Clone)]
pub struct OwnedObjectIterator {
    remaining: usize,
    i: i32,
    obj: JVRaw,
}

impl Iterator for OwnedObjectIterator {
    // Returning a JVString is the only way we can avoid copying. &str is impossible because that
    // would require borrowing from the iterator.
    type Item = (JVString, JV);
    fn next(&mut self) -> Option<Self::Item> {
        if unsafe { jv_object_iter_valid(self.obj.ptr, self.i) } == 0 {
            return None;
        }
        let k_raw = JVRaw {
            ptr: unsafe { jv_object_iter_key(self.obj.ptr, self.i) },
        };
        let v_raw = JVRaw {
            ptr: unsafe { jv_object_iter_value(self.obj.ptr, self.i) },
        };
        // If we wanted to live dangerously, we could say something like this:
        // Because jv values are COW, k's string value will stay valid as long as obj lives,
        // so we can return a &'a str. That's too spooky for now though.
        self.i = unsafe { jv_object_iter_next(self.obj.ptr, self.i) };
        self.remaining -= 1;
        let k = if let Ok(JV::String(k)) = k_raw.try_into() {
            k
        } else {
            panic!("Object keys must be strings");
        };
        Some((
            k,
            v_raw
                .try_into()
                .expect("Object should not contain invalid JV"),
        ))
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.remaining, Some(self.remaining))
    }
}

impl<'a> ExactSizeIterator for OwnedObjectIterator {}

pub struct ObjectValuesIterator<'a> {
    remaining: usize,
    i: i32,
    obj: &'a JVRaw,
}

impl<'a> Iterator for ObjectValuesIterator<'a> {
    type Item = JV;
    fn next(&mut self) -> Option<Self::Item> {
        if unsafe { jv_object_iter_valid(self.obj.ptr, self.i) } == 0 {
            return None;
        }
        let v = JVRaw {
            ptr: unsafe { jv_object_iter_value(self.obj.ptr, self.i) },
        };
        self.i = unsafe { jv_object_iter_next(self.obj.ptr, self.i) };
        self.remaining -= 1;
        Some(v.try_into().expect("Object should not contain invalid JV"))
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.remaining, Some(self.remaining))
    }
}

impl<'a> ExactSizeIterator for ObjectValuesIterator<'a> {}
