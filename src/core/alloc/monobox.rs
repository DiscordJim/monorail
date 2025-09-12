use std::{mem::MaybeUninit, pin::Pin};

use crate::core::alloc::{foreign::{create_foreign_ptr, transform_foreign, Mono}, Foreign};


#[allow(type_alias_bounds)]
pub type MonoBox<T: ?Sized> = Mono<Box<T>>;


impl<T> Mono<Box<T>> {
    #[inline]
    pub fn new(x: T) -> Self {
        Mono::from(Box::new(x))
    }
    #[inline]
    pub fn new_uninit() -> Mono<Box<MaybeUninit<T>>> {
        Mono::from(Box::new_uninit())
    }
    #[inline]
    pub fn pin(x: T) -> Pin<Mono<Box<T>>> {
        MonoBox::into_pin(Mono::from(Box::new(x)))
    }
}



impl<T> Mono<Box<[T]>> {
    #[inline]
    pub fn new_uninit_slice(len: usize) -> MonoBox<[MaybeUninit<T>]> {
        Mono::from(Box::new_uninit_slice(len))
    }
}

impl<T> MonoBox<MaybeUninit<T>> {
    #[inline]
    pub unsafe fn assume_init(self) -> MonoBox<T> {
        Mono::from(unsafe { self.into_inner().assume_init() })
    }
    #[inline]
    pub fn write(mut boxed: Self, value: T) -> MonoBox<T> {
        let boxed = boxed.into_inner();
        // boxed.write(val)
        Box::<MaybeUninit<T>>::write(boxed, value).into()
        // Mono::from(boxed.into_inner().write(value))
    }

}

impl<T> Foreign<Box<MaybeUninit<T>>>
where 
    T: Send
{
    #[inline]
    pub unsafe fn assume_init(self) -> Foreign<Box<T>> {
        transform_foreign(self, |f| unsafe { f.assume_init() })
    }
    #[inline]
    pub fn write(boxed: Self, value: T) -> Foreign<Box<T>> {
        transform_foreign(boxed, |f| MonoBox::write(f, value))
    }
}

impl<T> MonoBox<[MaybeUninit<T>]> {
    #[inline]
    pub unsafe fn assume_init(self) -> MonoBox<[T]> {
        Mono::from(unsafe { self.into_inner().assume_init()  })
    }
}

impl<T> Foreign<Box<[MaybeUninit<T>]>>
where 
    T: Send
{
    #[inline]
    pub unsafe fn assume_init(self) -> Foreign<Box<[T]>> {
        transform_foreign(self, |f| unsafe { f.assume_init() })
    }
}

impl<T: ?Sized> MonoBox<T> {
    #[inline]
    pub unsafe fn from_raw(raw: *mut T) -> Self {
        Mono::from(unsafe { Box::from_raw(raw)  })
    }
    #[inline]
    pub fn into_raw(b: Self) -> *mut T {
        Box::into_raw(b.into_inner())
    }
    #[inline]
    pub fn leak(b: Self) -> &'static mut T {
        Box::leak(b.into_inner())
    }
    #[inline]
    pub fn into_pin(boxed: Self) -> Pin<Self> {
        unsafe { Pin::new_unchecked(boxed) }
    }
}

impl<T: ?Sized> Foreign<Box<T>> {
    #[inline]
    pub fn into_pin(boxed: Self) -> Pin<Self> {
        unsafe { Pin::new_unchecked(boxed) }
    }
}

impl<T: ?Sized> Unpin for MonoBox<T> {}