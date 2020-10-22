use jq_sys::{
    jv, jv_array, jv_array_get, jv_array_length, jv_array_set, jv_bool, jv_copy, jv_equal, jv_free,
    jv_get_kind, jv_invalid_get_msg, jv_invalid_has_msg, jv_kind_JV_KIND_ARRAY,
    jv_kind_JV_KIND_FALSE, jv_kind_JV_KIND_INVALID, jv_kind_JV_KIND_NULL, jv_kind_JV_KIND_NUMBER,
    jv_kind_JV_KIND_OBJECT, jv_kind_JV_KIND_STRING, jv_kind_JV_KIND_TRUE, jv_null, jv_number,
    jv_number_value, jv_object, jv_object_iter, jv_object_iter_key, jv_object_iter_next,
    jv_object_iter_valid, jv_object_iter_value, jv_object_length, jv_object_set, jv_parse_sized,
    jv_string_length_bytes, jv_string_sized, jv_string_value,
};
use serde::{
    de::{MapAccess, SeqAccess, Visitor},
    Deserialize,
};
use serde_json::value::Value;
use std::{
    convert::{TryFrom, TryInto},
    fmt,
    iter::FromIterator,
    mem::forget,
    os::raw::c_char,
    slice, str,
};

pub(super) struct JVRaw {
    pub ptr: jv,
}
impl fmt::Debug for JVRaw {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "JV{{..}}")
    }
}

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
        let slice = unsafe {
            let string_ptr = jv_string_value(self.ptr) as *const u8;
            let len = jv_string_length_bytes(self.clone().unwrap_without_drop());
            slice::from_raw_parts(
                string_ptr,
                len.try_into().expect("length cannot be parsed as usize"),
            )
        };
        str::from_utf8(slice).expect("JQ strings are supposed to be UTF-8")
    }
    pub fn object_len(&self) -> i32 {
        unsafe { jv_object_length(self.clone().unwrap_without_drop()) }
    }
    pub fn object_iter(&self) -> impl ExactSizeIterator<Item = (String, JVRaw)> + '_ {
        let i = unsafe { jv_object_iter(self.ptr) };
        ObjectIterator { i, obj: self }
    }
    pub fn array_len(&self) -> i32 {
        unsafe { jv_array_length(self.clone().unwrap_without_drop()) }
    }
    pub fn array_get(&self, i: i32) -> JVRaw {
        JVRaw {
            ptr: unsafe { jv_array_get(self.clone().unwrap_without_drop(), i) },
        }
    }
    pub fn array_iter(&self) -> impl ExactSizeIterator<Item = JVRaw> + '_ {
        let len = self.array_len();
        (0..len).into_iter().map(move |i| self.array_get(i))
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
    pub fn to_serde(&self) -> Result<Value, String> {
        match self.get_kind() {
            JVKind::Invalid => Err(self
                .clone()
                .get_invalid_msg()
                .unwrap_or_else(|| "No error message".to_owned())),
            JVKind::Null => Ok(Value::Null),
            JVKind::False => Ok(Value::Bool(false)),
            JVKind::True => Ok(Value::Bool(true)),
            JVKind::Number => Ok(self.number_value().into()),
            JVKind::String => Ok(self.string_value().into()),
            JVKind::Array => Ok(self
                .array_iter()
                .map(|x| x.to_serde().expect("Array element should not be invalid"))
                .collect()),
            JVKind::Object => Ok(Value::Object(
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
    pub fn parse_native(s: &str) -> Self {
        JVRaw {
            ptr: unsafe {
                jv_parse_sized(s.as_ptr() as *const c_char, s.len().try_into().unwrap())
            },
        }
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

struct ObjectIterator<'a> {
    i: i32,
    obj: &'a JVRaw,
}

impl<'a> Iterator for ObjectIterator<'a> {
    type Item = (String, JVRaw);
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
        Some((k.string_value().into(), v))
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        let len = self.obj.object_len() as usize;
        (len, Some(len))
    }
}

impl<'a> ExactSizeIterator for ObjectIterator<'a> {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JVNull(JVRaw);
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JVBool(JVRaw);
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JVNumber(JVRaw);
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JVString(JVRaw);
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JVArray(JVRaw);
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JVObject(JVRaw);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JV {
    Null(JVNull),
    Bool(JVBool),
    Number(JVNumber),
    String(JVString),
    Array(JVArray),
    Object(JVObject),
}
impl JVNull {
    pub fn new() -> Self {
        JVNull(JVRaw::null())
    }
}
impl JVBool {
    pub fn new(b: bool) -> Self {
        JVBool(JVRaw::bool(b))
    }
    pub fn value(&self) -> bool {
        match self.0.get_kind() {
            JVKind::True => true,
            JVKind::False => false,
            _ => panic!("Invalid kind fo JVBool"),
        }
    }
}
impl JVNumber {
    pub fn new(x: f64) -> Self {
        JVNumber(JVRaw::number(x))
    }
    pub fn value(&self) -> f64 {
        self.0.number_value()
    }
}
impl JVString {
    pub fn new(s: &str) -> Self {
        JVString(JVRaw::string(s))
    }
    pub fn value(&self) -> &str {
        self.0.string_value()
    }
}
impl JVArray {
    pub fn new() -> Self {
        JVArray(JVRaw::empty_array())
    }
    pub fn set(&mut self, i: i32, v: JV) {
        self.0.array_set(i, v.into())
    }
    pub fn iter(&self) -> impl ExactSizeIterator<Item = JV> + '_ {
        self.0.array_iter().map(|v| {
            v.try_into()
                .expect("JV should not have nested invalid value")
        })
    }
    pub fn len(&self) -> i32 {
        self.0.array_len()
    }
    pub fn get(&self, i: i32) -> Option<JV> {
        if (0..self.len()).contains(&i) {
            Some(
                self.0
                    .array_get(i)
                    .try_into()
                    .expect("JV should not have nested invalid value"),
            )
        } else {
            None
        }
    }
}
impl JVObject {
    pub fn new() -> Self {
        JVObject(JVRaw::empty_object())
    }
    pub fn set(&mut self, k: &str, v: JV) {
        self.0.object_set(k, v.into())
    }
    pub fn iter(&self) -> impl ExactSizeIterator<Item = (String, JV)> + '_ {
        self.0.object_iter().map(|(k, v)| {
            (
                k,
                v.try_into()
                    .expect("JV should not have nested invalid value"),
            )
        })
    }
}
impl From<JVNull> for JV {
    fn from(x: JVNull) -> Self {
        JV::Null(x)
    }
}
impl From<JVBool> for JV {
    fn from(x: JVBool) -> Self {
        JV::Bool(x)
    }
}
impl From<JVNumber> for JV {
    fn from(x: JVNumber) -> Self {
        JV::Number(x)
    }
}
impl From<JVString> for JV {
    fn from(x: JVString) -> Self {
        JV::String(x)
    }
}
impl From<JVArray> for JV {
    fn from(x: JVArray) -> Self {
        JV::Array(x)
    }
}
impl From<JVObject> for JV {
    fn from(x: JVObject) -> Self {
        JV::Object(x)
    }
}
impl TryFrom<JVRaw> for JV {
    type Error = String;

    fn try_from(raw: JVRaw) -> Result<Self, Self::Error> {
        match raw.get_kind() {
            JVKind::Invalid => Err(raw
                .clone()
                .get_invalid_msg()
                .unwrap_or_else(|| "No error message".to_owned())),
            JVKind::Null => Ok(JVNull(raw).into()),
            JVKind::False | JVKind::True => Ok(JVBool(raw).into()),
            JVKind::Number => Ok(JVNumber(raw).into()),
            JVKind::String => Ok(JVString(raw).into()),
            JVKind::Array => Ok(JVArray(raw).into()),
            JVKind::Object => Ok(JVObject(raw).into()),
        }
    }
}
impl TryFrom<&JV> for Value {
    type Error = String;

    fn try_from(j: &JV) -> Result<Self, Self::Error> {
        let raw: &JVRaw = j.into();
        raw.to_serde()
    }
}
impl From<&Value> for JV {
    fn from(v: &Value) -> Self {
        JVRaw::from_serde(v)
            .try_into()
            .expect("from_serde should not produce invalid value")
    }
}
impl<'a> From<&'a JV> for &'a JVRaw {
    fn from(j: &'a JV) -> Self {
        match j {
            &JV::Null(JVNull(ref out))
            | &JV::Bool(JVBool(ref out))
            | &JV::Number(JVNumber(ref out))
            | &JV::String(JVString(ref out))
            | &JV::Array(JVArray(ref out))
            | &JV::Object(JVObject(ref out)) => out,
        }
    }
}
impl From<JV> for JVRaw {
    fn from(j: JV) -> Self {
        match j {
            JV::Null(JVNull(out))
            | JV::Bool(JVBool(out))
            | JV::Number(JVNumber(out))
            | JV::String(JVString(out))
            | JV::Array(JVArray(out))
            | JV::Object(JVObject(out)) => out,
        }
    }
}

impl<'de> Deserialize<'de> for JV {
    #[inline]
    fn deserialize<D>(deserializer: D) -> Result<JV, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct JVVisitor;

        impl<'de> Visitor<'de> for JVVisitor {
            type Value = JV;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("any valid JSON value")
            }

            #[inline]
            fn visit_bool<E>(self, value: bool) -> Result<JV, E> {
                Ok(JVBool::new(value).into())
            }

            #[inline]
            fn visit_i64<E>(self, value: i64) -> Result<JV, E> {
                Ok(JVNumber::new(value as f64).into())
            }

            #[inline]
            fn visit_u64<E>(self, value: u64) -> Result<JV, E> {
                Ok(JVNumber::new(value as f64).into())
            }

            #[inline]
            fn visit_f64<E>(self, value: f64) -> Result<JV, E> {
                Ok(JVNumber::new(value).into())
            }

            fn visit_str<E>(self, value: &str) -> Result<JV, E> {
                Ok(JVString::new(value).into())
            }

            fn visit_string<E>(self, value: String) -> Result<JV, E> {
                Ok(JVString::new(&value).into())
            }

            #[inline]
            fn visit_none<E>(self) -> Result<JV, E> {
                Ok(JVNull::new().into())
            }

            #[inline]
            fn visit_some<D>(self, deserializer: D) -> Result<JV, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                Deserialize::deserialize(deserializer)
            }

            #[inline]
            fn visit_unit<E>(self) -> Result<JV, E> {
                Ok(JVNull::new().into())
            }

            #[inline]
            fn visit_seq<V>(self, mut visitor: V) -> Result<JV, V::Error>
            where
                V: SeqAccess<'de>,
            {
                let mut i = 0;
                let mut arr = JVArray::new();

                while let Some(elem) = visitor.next_element()? {
                    arr.set(i, elem);
                    i += 1;
                }

                Ok(arr.into())
            }

            fn visit_map<V>(self, mut visitor: V) -> Result<JV, V::Error>
            where
                V: MapAccess<'de>,
            {
                let mut obj = JVObject::new();

                while let Some((key, value)) = visitor.next_entry::<String, _>()? {
                    obj.set(&key, value);
                }

                Ok(obj.into())
            }
        }

        deserializer.deserialize_any(JVVisitor)
    }
}

impl JV {
    pub fn parse_native(s: &str) -> Result<Self, String> {
        JVRaw::parse_native(s).try_into()
    }
}
#[cfg(test)]
mod tests {
    use super::JV;
    use crate::testing::arb_json;
    use proptest::proptest;
    use serde_json::{json, value::Value};
    use std::convert::TryInto;
    fn test_jv_roundtrip(value: Value) {
        let jv: JV = (&value).into();
        let roundtrip: Value = (&jv).try_into().unwrap();
        assert_eq!(value, roundtrip);
    }
    #[test]
    fn null_jv_roundtrip() {
        test_jv_roundtrip(json!(null));
    }
    #[test]
    fn bool_jv_roundtrip() {
        test_jv_roundtrip(json!(true));
    }
    #[test]
    fn string_jv_roundtrip() {
        test_jv_roundtrip(json!("hello"));
    }
    #[test]
    fn number_jv_roundtrip() {
        test_jv_roundtrip(json!(42.0));
    }
    #[test]
    fn array_jv_roundtrip() {
        test_jv_roundtrip(json!([1.0, 2.0, 3.0]));
    }
    #[test]
    fn object_jv_roundtrip() {
        test_jv_roundtrip(json!({"key":"value"}));
    }
    proptest! {
        #[test]
        fn prop_jv_roundtrip(value in arb_json()) {
            test_jv_roundtrip(value);
        }
    }
    proptest! {
        #[test]
        fn prop_jv_deserialize(value in arb_json()) {
            let s = serde_json::to_string(&value)?;
            let jv : JV= serde_json::from_str(&s)?;
            let via_jv: Value = (&jv).try_into().unwrap();
            let via_str: Value = serde_json::from_str(&s)?;
            assert_eq!(via_jv, via_str);
        }
    }
}
