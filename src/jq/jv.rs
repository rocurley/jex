use super::jv_raw::{JVKind, JVRaw};
pub use super::jv_raw::{ObjectIterator, ObjectValuesIterator, OwnedObjectIterator};
use serde::{
    de::{MapAccess, SeqAccess, Visitor},
    ser::{SerializeMap, SerializeSeq},
    Deserialize, Serialize, Serializer,
};
use serde_json::value::Value;
use std::{
    convert::{TryFrom, TryInto},
    fmt,
};

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
            _ => panic!("Invalid kind for JVBool"),
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
    pub fn iter(&self) -> BorrowedArrayIterator<'_> {
        BorrowedArrayIterator { i: 0, arr: self }
    }
    pub fn len(&self) -> i32 {
        self.0.array_len()
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    pub fn get(&self, i: i32) -> Option<JV> {
        if (0..self.len()).contains(&i) {
            let raw = self.0.array_get(i);
            Some(
                raw.try_into()
                    .expect("JV should not have nested invalid value"),
            )
        } else {
            None
        }
    }
}

pub struct OwnedArrayIterator {
    i: i32,
    arr: JVArray,
}
impl Iterator for OwnedArrayIterator {
    type Item = JV;
    fn next(&mut self) -> Option<Self::Item> {
        let out = self.arr.get(self.i)?;
        self.i += 1;
        Some(out)
    }
}

pub struct BorrowedArrayIterator<'a> {
    i: i32,
    arr: &'a JVArray,
}
impl<'a> Iterator for BorrowedArrayIterator<'a> {
    type Item = JV;
    fn next(&mut self) -> Option<Self::Item> {
        let out = self.arr.get(self.i)?;
        self.i += 1;
        Some(out)
    }
}

impl std::iter::IntoIterator for JVArray {
    type Item = JV;
    type IntoIter = OwnedArrayIterator;
    fn into_iter(self) -> Self::IntoIter {
        OwnedArrayIterator { i: 0, arr: self }
    }
}
impl JVObject {
    pub fn new() -> Self {
        JVObject(JVRaw::empty_object())
    }
    pub fn set(&mut self, k: &str, v: JV) {
        self.0.object_set(k, v.into())
    }
    pub fn iter(&self) -> ObjectIterator {
        self.0.object_iter()
    }
    pub fn values(&self) -> ObjectValuesIterator {
        self.0.object_values()
    }
    pub fn len(&self) -> i32 {
        self.0.object_len()
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    pub fn into_empty_iter(self) -> OwnedObjectIterator {
        self.0.into_empty_object_iter()
    }
}
impl std::iter::IntoIterator for JVObject {
    type Item = (JVString, JV);
    type IntoIter = OwnedObjectIterator;
    fn into_iter(self) -> Self::IntoIter {
        self.0.into_object_iter()
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
impl From<&JV> for Value {
    fn from(j: &JV) -> Self {
        match j {
            JV::Null(_) => Value::Null,
            JV::Bool(b) => b.value().into(),
            JV::Number(x) => x.value().into(),
            JV::String(s) => s.value().into(),
            JV::Array(arr) => arr.iter().map(|x| Value::from(&x)).collect(),
            JV::Object(obj) => Value::Object(
                obj.iter()
                    .map(|(k, v)| (k.to_owned(), (&v).into()))
                    .collect(),
            ),
        }
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

impl Default for JVNull {
    fn default() -> Self {
        Self::new()
    }
}

impl Default for JVArray {
    fn default() -> Self {
        Self::new()
    }
}

impl Default for JVObject {
    fn default() -> Self {
        Self::new()
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

impl Serialize for JV {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            JV::Null(_) => serializer.serialize_none(),
            JV::Bool(b) => serializer.serialize_bool(b.value()),
            JV::Number(x) => serializer.serialize_f64(x.value()),
            JV::String(s) => serializer.serialize_str(s.value()),
            JV::Array(arr) => {
                let mut seq = serializer.serialize_seq(Some(arr.len() as usize))?;
                for x in arr.iter() {
                    seq.serialize_element(&x)?;
                }
                seq.end()
            }
            JV::Object(obj) => {
                let mut map = serializer.serialize_map(Some(obj.len() as usize))?;
                for (k, v) in obj.iter() {
                    map.serialize_entry(&k, &v)?;
                }
                map.end()
            }
        }
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
    proptest! {
        #[test]
        fn prop_jv_serialize(value in arb_json()) {
            let jv :JV = (&value).into();
            let s1 = serde_json::to_string(&jv)?;
            let s2 = serde_json::to_string(&value)?;
            assert_eq!(s1, s2);
            // Distressingly, serde doesn't actually pass the roundtrip test. Gonna let this one go
            // for now.
            /*
            let via_ser: Value = serde_json::from_str(&s2)?;
            assert_eq!(value, via_ser, "String was : {:?}", s2);
            */
        }
    }
}
