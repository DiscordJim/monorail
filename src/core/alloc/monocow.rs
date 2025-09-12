use std::borrow::Cow;

use crate::core::alloc::{foreign::Mono, Foreign};



pub type MonoCow<'a, B> = Mono<Cow<'a, B>>;



impl<'a, B> MonoCow<'a, B>
where 
    B: ?Sized + ToOwned
{
    #[inline]
    pub fn is_borrowed(&self) -> bool {
        match &**self {
            Cow::Borrowed(_) => true,
            _ => false
        }
    }
    #[inline]
    pub fn is_owned(&self) -> bool {
        !self.is_borrowed()
    }
    #[inline]
    pub fn into_owned(self) -> Mono<<B as ToOwned>::Owned> {
        Mono::from(self.into_inner().into_owned())
    }
}

// impl<'a, B> Foreign<Cow<'a, B>>
// where 
//     B: ?Sized + ToOwned
// {
//     #[inline]
//     pub fn into_owned(self) -> Cow<'a, Foreign<>> {
        
        
//     }
// }
