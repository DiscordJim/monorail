use std::collections::LinkedList;

use crate::core::alloc::foreign::Mono;



pub type MonoLinkedList<T> = Mono<LinkedList<T>>;

impl<T> MonoLinkedList<T> {
    #[inline]
    pub const fn new() -> Self {
        Mono::from_const(LinkedList::new())
    }
}