use std::{borrow::Cow, marker::PhantomData, ops::{Deref, DerefMut}, rc::Rc};

use crate::core::alloc::{foreign::{create_foreign_ptr, transform_foreign, Mono}, Foreign, MonoBox};

#[macro_export]
macro_rules! monovec {
    () => (
        $crate::core::alloc::Mono::from_const(vec![])
        // $crate::core::alloc::MonoVec::new()
    );
    ($elem:expr; $n:expr) => (
        $crate::core::alloc::Mono::from_const(vec![$elem:expr; $n:expr])
        // $crate::vec::from_elem($elem, $n)
    );
    ($($x:expr),+ $(,)?) => (
        // <[_]>::into_vec(
            // Using the intrinsic produces a dramatic improvement in stack usage for
            // unoptimized programs using this code path to construct large Vecs.
            // $crate::boxed::box_new([$($x),+])
        // )
        $crate::core::alloc::Mono::from_const(vec![$($x),+])
    );
}


// fn hi() {
//     // let v = monovec![4u8, 2, 3, 4usize];
// }

pub type MonoVec<T> = Mono<Vec<T>>;


impl<T> Mono<Vec<T>> {
    #[inline]
    pub const fn new() -> Self {
        Mono::from_const(Vec::new())
    }
    #[inline]
    pub fn with_capacity(capacity: usize) -> Self {
        Mono::from(Vec::with_capacity(capacity))
    }
    #[inline]
    pub unsafe fn from_raw_parts(ptr: *mut T, length: usize, capacity: usize) -> Self {
        Mono::from(unsafe { Vec::from_raw_parts(ptr, length, capacity) })
    }
    #[inline]
    pub fn into_boxed_slice(self) -> Mono<Box<[T]>> {
        Mono::from(self.into_inner().into_boxed_slice())
    }
    #[inline]
    pub fn leak(self) -> &'static mut [T] {
        Vec::leak(self.into_inner())
    }
}

impl<T, const N: usize> Mono<Vec<[T; N]>> {
    pub fn into_flattened(self) -> Mono<Vec<T>> {
        Mono::from(self.into_inner().into_flattened())
    }
}

impl<T> Foreign<Vec<T>>
where 
    T: Send
{
    #[inline]
    pub fn into_boxed_slice(self) -> Foreign<Box<[T]>> {
        transform_foreign(self, |f| f.into_boxed_slice())
        // create_foreign_ptr(extract_foreign_ptr(self).into_boxed_slice())
    }
}

impl<T, const N: usize> Foreign<Vec<[T; N]>>
where 
    T: Send
{
    #[inline]
    pub fn into_flattened(self) -> Foreign<Vec<T>> {
        transform_foreign(self, |f| f.into_flattened())
        // create_foreign_ptr(extract_foreign_ptr(self).into_flattened())
    }
}

// pub struct MonoVec<T> {
//     inner: Vec<T>,
//     _marker: PhantomData<Rc<()>>
// }

// impl<T> From<Vec<T>> for MonoVec<T> {
//     fn from(value: Vec<T>) -> Self {
//         create_monovec(value)
//     }
// }

// #[inline]
// const fn create_monovec<T>(vec: Vec<T>) -> MonoVec<T> {
//     MonoVec {
//         inner: vec,
//         _marker: PhantomData
//     }
// }

// // #[macro]

// impl<T> MonoVec<T> {
//     #[inline]
//     pub const fn new() -> Self {
//         create_monovec(Vec::new())
//     }
//     #[inline]
//     pub fn with_capacity(capacity: usize) -> Self {
//         create_monovec(Vec::with_capacity(capacity))
//     }
//     #[inline]
//     pub unsafe fn from_raw_parts(ptr: *mut T, length: usize, capacity: usize) -> Self {
//         create_monovec(Vec::from_raw_parts(ptr, length, capacity))
//     }
//     #[inline]
//     pub fn into_boxed_slice(self) -> MonoBox<[T]> {
//         MonoBox::from(self.inner.into_boxed_slice())
//     }
//     pub fn leak(self) -> &'static [T] {
//         Vec::leak(self.inner)
//     }
//     // pub fn reserve(&mut self, additiona)
// }



// impl<T> Foreign<MonoVec<T>>
// where 
//     T: Send
// {
//     pub fn into_boxed_slice(self) -> Foreign<MonoBox<[T]>> {
//         transform_foreign::<MonoVec<T>, MonoBox<[T]>>(self, |f| MonoVec::into_boxed_slice(f))
//     }
// }

// impl<T, const N: usize> Foreign<MonoVec<[T; N]>>
// where
//     T: Send,
//     // Foreign<Mo>
// {
//     pub fn into_flattened(self) -> Foreign<MonoVec<T>> {
//         transform_foreign(self, |f| MonoVec::into_flattened(f))
//     }
// }

// impl<T, const N: usize> MonoVec<[T; N]> {
//     pub fn into_flattened(self) -> MonoVec<T> {
//         create_monovec(Vec::into_flattened(self.inner))
//     }
// }

// impl<T: Clone> Clone for MonoVec<T> {
//     fn clone(&self) -> Self {
//         create_monovec(self.inner.clone())
//     }
// }

// // impl

// impl<T> FromIterator<T> for MonoVec<T> {
//     #[inline]
//     fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
//         let v = Vec::from_iter(iter);
//         create_monovec(v)
//     }
// }

// impl<T> Deref for MonoVec<T> {
//     type Target = Vec<T>;
//     fn deref(&self) -> &Self::Target {
//         &self.inner
//     }
// }

// impl<T> DerefMut for MonoVec<T> {
//     fn deref_mut(&mut self) -> &mut Self::Target {
//         &mut self.inner
//     }
// }

// impl<T> IntoIterator for MonoVec<T> {
//     type Item = T;
//     type IntoIter = std::vec::IntoIter<T>;
//     fn into_iter(self) -> Self::IntoIter {
//         self.inner.into_iter()
//     }
// }

// impl<T: Send> IntoIterator for Foreign<MonoVec<T>> {
//     type Item = T;
//     type IntoIter = Foreign<std::vec::IntoIter<T>>;
//     fn into_iter(self) -> Self::IntoIter {
//         let e = super::foreign::extract_foreign_ptr(self).inner.into_iter();
//         create_foreign_ptr(e)
//     }
// }


// impl<T> Default for MonoVec<T> {
//     #[inline]
//     fn default() -> Self {
//         create_monovec(Vec::default())
//     }
// }

// impl<T: Clone> From<&[T]> for MonoVec<T> {
//     #[inline]
//     fn from(value: &[T]) -> Self {
//         create_monovec(value.to_vec())
//     }
// }

// impl<T: Clone> From<&mut [T]> for MonoVec<T> {
//     #[inline]
//     fn from(value: &mut [T]) -> Self {
//         create_monovec(value.to_vec())
//     }
// }

// impl<T: Clone, const N: usize> From<&[T; N]> for MonoVec<T> {
//     #[inline]
//     fn from(value: &[T; N]) -> Self {
//         create_monovec(value.to_vec())
//     }
// }

// impl<T: Clone, const N: usize> From<&mut [T; N]> for MonoVec<T> {
//     #[inline]
//     fn from(value: &mut [T; N]) -> Self {
//         create_monovec(value.to_vec())
//     }
// }

// impl<T: Clone, const N: usize> From<[T; N]> for MonoVec<T> {
//     #[inline]
//     fn from(value: [T; N]) -> Self {
//         create_monovec(value.to_vec())
//     }
// }

// impl<'a, T> From<Cow<'a, [T]>> for MonoVec<T>
// where 
//     [T]: ToOwned<Owned = MonoVec<T>>

// {

//     fn from(value: Cow<'a, [T]>) -> Self {
//         value.into_owned()
//     }

// }

// impl<T> From<MonoBox<[T]>> for MonoVec<T> {
//     fn from(value: MonoBox<[T]>) -> Self {
//         // let d = std::
//         // let d = &**value;
//         value.into_vec()
//     }
// }


// impl<T> From<Foreign<MonoBox<[T]>>> for Foreign<MonoVec<T>> 
// where   
//     // Foreign<MonoBox<[T]>>: Send,
//     T: Send
// {
//     fn from(value: Foreign<MonoBox<[T]>>) -> Self {
//         transform_foreign::<MonoBox<[T]>, MonoVec<T>>(value, |t| MonoVec::from(t))
//     }
// }

// impl<T> From<Box<[T]>> for MonoVec<T> {
//     fn from(value: Box<[T]>) -> Self {
//         create_monovec(value.into_vec())
//     }
// }

// impl<T> From<MonoVec<T>> for MonoBox<[T]> {
//     fn from(value: MonoVec<T>) -> Self {
//         value.into_boxed_slice()
//     }
// }

// impl<T> From<Foreign<MonoVec<T>>> for Foreign<MonoBox<[T]>>
// where 
//     // Foreign<MonoBox<[T]>>: Send,
//     T: Send
// {
//     fn from(value: Foreign<MonoVec<T>>) -> Self {
//         transform_foreign::<MonoVec<T>, MonoBox<[T]>>(value, |t| MonoBox::from(t))
//     }
// }

// impl From<&str> for MonoVec<u8> {
//     fn from(value: &str) -> Self {
//         From::from(value.as_bytes())
//     }
// }

// impl<T, const N: usize> TryFrom<MonoVec<T>> for [T; N] {
//     type Error = MonoVec<T>;

//     fn try_from(value: MonoVec<T>) -> Result<Self, Self::Error> {
//         TryFrom::try_from(value.inner).map_err(create_monovec)
//     }
// }

// // impl