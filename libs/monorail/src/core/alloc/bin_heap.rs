use std::collections::BinaryHeap;

use crate::core::alloc::{foreign::{transform_foreign, Mono}, monovec::MonoVec, Foreign};



pub type MonoBinaryHeap<T> = Mono<BinaryHeap<T>>;

impl<T> MonoBinaryHeap<T>
where 
    T: Ord
{
    #[inline]
    pub const fn new() -> MonoBinaryHeap<T> {
        Mono::from_const(BinaryHeap::new())
    }

    #[inline]
    pub fn with_capacity(capacity: usize) -> Self {
        Mono::from_const(BinaryHeap::with_capacity(capacity))
    }
    #[inline]
    pub fn into_sorted_vec(self) -> MonoVec<T> {
        self.project(BinaryHeap::into_sorted_vec)
    }
    #[inline]
    pub fn into_vec(self) -> MonoVec<T> {
        self.project(BinaryHeap::into_vec)
    }
}

impl<T> Foreign<BinaryHeap<T>>
where 
    T: Ord,
    BinaryHeap<T>: Send
{
    #[inline]
    pub fn into_sorted_vec(self) -> Foreign<Vec<T>>
    where 
        T: Send
    {
        transform_foreign(self, Mono::<BinaryHeap<T>>::into_sorted_vec)
    }
    #[inline]
    pub fn into_vec(self) -> Foreign<Vec<T>>
    where 
        T: Send
    {
        transform_foreign(self, Mono::<BinaryHeap<T>>::into_vec)
    }
}