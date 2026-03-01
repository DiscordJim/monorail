use std::string::FromUtf8Error;

use crate::core::alloc::{foreign::{transform_foreign, Mono}, monovec::MonoVec, Foreign, MonoBox};



pub type MonoString = Mono<String>;


impl MonoString {

    #[inline]
    pub const fn new() -> MonoString {
        Mono::from_const(String::new())
    }

    #[inline]
    pub fn with_capacity(capacity: usize) -> MonoString {
        Mono::from_const(String::with_capacity(capacity))
    }

    #[inline]
    pub fn from_utf8(vec: MonoVec<u8>) -> Result<MonoString, FromUtf8Error> {
        Ok(Mono::from_const(vec.project(String::from_utf8).into_inner()?))
    }

    #[inline]
    pub unsafe fn from_raw_parts(buf: *mut u8, length: usize, capacity: usize) -> MonoString {
        Mono::from_const(unsafe { String::from_raw_parts(buf, length, capacity) })
    }

    #[inline]
    pub unsafe fn from_utf8_unchecked(bytes: MonoVec<u8>) -> MonoString {
        bytes.project(|f| unsafe { String::from_utf8_unchecked(f) })
    }

    #[inline]
    pub fn into_bytes(self) -> MonoVec<u8> {
        Mono::from_const(self.into_inner().into_bytes())
    }

    // #[inline]
    #[inline]
    pub fn into_boxed_str(self) -> MonoBox<str> {
        self.project(String::into_boxed_str)
    }

    #[inline]
    pub fn leak(self) -> &'static mut str {
        String::leak(self.into_inner())
    }

 
}

impl Foreign<String> {
    #[inline]
    pub fn from_utf8(vec: Foreign<Vec<u8>>) -> Result<Foreign<String>, FromUtf8Error> {
        transform_foreign::<Vec<u8>, Result<String, FromUtf8Error>>(vec, |f| {
            // let s = MonoString::from_utf8(f)?;

            // Mono::from_const(Ok(s.into_inner()))
            match MonoString::from_utf8(f) {
                Ok(v) => Mono::from_const(Ok(v.into_inner())),
                Err(e) => Mono::from_const(Err(e))
            }
        }).invert_result()
    }

    #[inline]
    pub fn into_boxed_str(self) -> Foreign<Box<str>> {
        transform_foreign(self, MonoString::into_boxed_str)
    }
    // #[inline]
    // pub fn leak(self) -> &'static mut str {
    //     // String::leak(s)
    // }
}