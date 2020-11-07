use super::{
    jv::{JVArray, JVBool, JVNull, JVNumber, JVObject, JVString, JV},
    jv_raw::{JVKind, JVRaw, JVRawBorrowed},
};
use jq_sys::jv;
use serde_json::value::Value;
use std::{convert::TryFrom, ops::Deref};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JVBorrowed<'a> {
    Null(JVNullBorrowed<'a>),
    Bool(JVBoolBorrowed<'a>),
    Number(JVNumberBorrowed<'a>),
    String(JVStringBorrowed<'a>),
    Array(JVArrayBorrowed<'a>),
    Object(JVObjectBorrowed<'a>),
}

// TODO: probably we should merge this file with jv.rs and move more stuff into this macro.
macro_rules! borrowed_jv {
    ($constructor:ident: $borrowed:ident; $owned:ty) => {
        #[repr(transparent)]
        #[derive(Debug, Clone, PartialEq, Eq)]
        pub struct $borrowed<'a>(JVRawBorrowed<'a>);

        impl<'a> $borrowed<'a> {
            // See safety discussion in JVRawBorrowed::deref
            unsafe fn deref<'b>(&'b self) -> &'a $owned {
                &*(&self.0.ptr as *const jv as *const $owned)
            }
        }
        impl<'a> Deref for $borrowed<'a> {
            type Target = $owned;
            fn deref(&self) -> &Self::Target {
                // See safety discussion in JVRawBorrowed
                unsafe { self.deref() }
            }
        }
        impl<'a> From<$borrowed<'a>> for JVBorrowed<'a> {
            fn from(x: $borrowed<'a>) -> Self {
                JVBorrowed::$constructor(x)
            }
        }
        impl $owned {
            pub fn borrow<'a>(&'a self) -> $borrowed<'a> {
                $borrowed(self.0.borrow())
            }
        }
    };
}

borrowed_jv!(Null: JVNullBorrowed; JVNull);
borrowed_jv!(Bool:JVBoolBorrowed; JVBool);
borrowed_jv!(Number:JVNumberBorrowed; JVNumber);
borrowed_jv!(String:JVStringBorrowed; JVString);
borrowed_jv!(Array:JVArrayBorrowed; JVArray);
borrowed_jv!(Object:JVObjectBorrowed; JVObject);

impl JV {
    pub fn borrow<'a>(&'a self) -> JVBorrowed<'a> {
        match self {
            JV::Null(x) => JVBorrowed::Null(x.borrow()),
            JV::Bool(x) => JVBorrowed::Bool(x.borrow()),
            JV::Number(x) => JVBorrowed::Number(x.borrow()),
            JV::String(x) => JVBorrowed::String(x.borrow()),
            JV::Array(x) => JVBorrowed::Array(x.borrow()),
            JV::Object(x) => JVBorrowed::Object(x.borrow()),
        }
    }
}

impl<'a> JVBorrowed<'a> {
    pub fn to_owned(self) -> JV {
        match self {
            JVBorrowed::Null(x) => JV::Null((*x).clone()),
            JVBorrowed::Bool(x) => JV::Bool((*x).clone()),
            JVBorrowed::Number(x) => JV::Number((*x).clone()),
            JVBorrowed::String(x) => JV::String((*x).clone()),
            JVBorrowed::Array(x) => JV::Array((*x).clone()),
            JVBorrowed::Object(x) => JV::Object((*x).clone()),
        }
    }
}
impl<'a> From<JVBorrowed<'a>> for Value {
    fn from(j: JVBorrowed<'a>) -> Self {
        match j {
            JVBorrowed::Null(n) => (&*n).into(),
            JVBorrowed::Bool(b) => (&*b).into(),
            JVBorrowed::Number(x) => (&*x).into(),
            JVBorrowed::String(s) => (&*s).into(),
            JVBorrowed::Array(arr) => (&*arr).into(),
            JVBorrowed::Object(obj) => (&*obj).into(),
        }
    }
}

impl<'a> TryFrom<JVRawBorrowed<'a>> for JVBorrowed<'a> {
    type Error = String;

    fn try_from(raw: JVRawBorrowed<'a>) -> Result<Self, Self::Error> {
        match raw.get_kind() {
            JVKind::Invalid => Err(JVRaw::clone(&raw)
                .get_invalid_msg()
                .unwrap_or_else(|| "No error message".to_owned())),
            JVKind::Null => Ok(JVNullBorrowed(raw).into()),
            JVKind::False | JVKind::True => Ok(JVBoolBorrowed(raw).into()),
            JVKind::Number => Ok(JVNumberBorrowed(raw).into()),
            JVKind::String => Ok(JVStringBorrowed(raw).into()),
            JVKind::Array => Ok(JVArrayBorrowed(raw).into()),
            JVKind::Object => Ok(JVObjectBorrowed(raw).into()),
        }
    }
}

impl<'a> JVStringBorrowed<'a> {
    pub fn value<'b>(&'b self) -> &'a str {
        // Safety: return of self.deref does not outlive this function, just the borrowed value.
        unsafe { self.deref().value() }
    }
}
