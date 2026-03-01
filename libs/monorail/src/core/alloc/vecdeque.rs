use std::collections::VecDeque;

use crate::core::alloc::foreign::Mono;




pub type MonoVecDeque<T> = Mono<VecDeque<T>>;


impl<T> MonoVecDeque<T> {
    #[inline]
    pub const fn new() -> MonoVecDeque<T> {
        Mono::from_const(VecDeque::new())
    }
    #[inline]
    pub fn with_capacity(capacity: usize) -> MonoVecDeque<T> {
        Mono::from_const(VecDeque::with_capacity(capacity))
    }
}