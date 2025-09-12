use std::{ffi::{CStr, CString, FromVecWithNulError, IntoStringError, NulError}, os::raw::c_char};

use crate::core::alloc::{foreign::Mono, monovec::MonoVec, str::string::MonoString, Foreign, MonoBox};



pub type MonoCString = Mono<CString>;

impl MonoCString {

    #[inline]
    pub fn new<T: Into<MonoVec<u8>>>(t: T) -> Result<MonoCString, NulError> {
        // let inner: Vec<u8> = t.into().into_inner();


        CString::new(t.into().into_inner()).map(Mono::from_const)

        // CString::new(inner)
    }

    #[inline]
    pub unsafe fn from_vec_unchecked(v: MonoVec<u8>) -> Self {
        v.project(|f| unsafe { CString::from_vec_unchecked(f) })
    }

    #[inline]
    pub unsafe fn from_raw(ptr: *mut c_char) -> MonoCString {
        Mono::from_const(CString::from_raw(ptr))
    } 

    #[inline]
    pub fn into_raw(self) -> *mut c_char {
        self.into_inner().into_raw()
    }

    #[inline]
    pub fn into_string(self) -> Result<MonoString, IntoStringError> {
        let str = self.into_inner().into_string()?;
        Ok(Mono::from_const(str))
    }

    #[inline]
    pub fn into_bytes(self) -> MonoVec<u8> {
        self.project(CString::into_bytes)
    }

    #[inline]
    pub fn into_bytes_with_nul(self) -> MonoVec<u8> {
        self.project(CString::into_bytes_with_nul)
    }

    #[inline]
    pub fn into_boxed_c_str(self) -> MonoBox<CStr> {
        self.project(CString::into_boxed_c_str)
    }
    #[inline]
    pub unsafe fn from_vec_with_nul_unchecked(v: MonoVec<u8>) -> Self {
        v.project(|f| unsafe { CString::from_vec_with_nul_unchecked(f) })
    }

    #[inline]
    pub fn from_vec_with_nul(v: MonoVec<u8>) -> Result<Self, FromVecWithNulError> {
        let mono = CString::from_vec_with_nul(v.into_inner())?;

        Ok(Mono::from_const(mono))
    }

}

impl Foreign<CString> {
    #[inline]
    pub fn into_string(self) -> Result<Foreign<String>, Foreign<IntoStringError>> {
        self.try_project(CString::into_string)
    }

    #[inline]
    pub fn into_bytes(self) -> Foreign<Vec<u8>> {
        self.project(CString::into_bytes)
    }

    #[inline]
    pub fn into_bytes_with_nul(self) -> Foreign<Vec<u8>> {
        self.project(CString::into_bytes_with_nul)
    }

    #[inline]
    pub fn into_boxed_c_str(self) -> Foreign<Box<CStr>> {
        self.project(CString::into_boxed_c_str)
    }

    #[inline]
    pub unsafe fn from_vec_with_nul_unchecked(v: Foreign<Vec<u8>>) -> Self {
        v.project(|f| unsafe { CString::from_vec_with_nul_unchecked(f) })
    }

    #[inline]
    pub fn from_vec_with_nul(v: Foreign<Vec<u8>>) -> Result<Foreign<CString>, Foreign<FromVecWithNulError>> {
        v.try_project(CString::from_vec_with_nul)
    }
}