use std::collections::BTreeSet;

use crate::core::alloc::foreign::Mono;



pub type MonoBTreeSet<T> = Mono<BTreeSet<T>>;


impl<T> MonoBTreeSet<T> {
    #[inline]
    pub const fn new() -> MonoBTreeSet<T> {
        Mono::from_const(BTreeSet::new())
    }
}