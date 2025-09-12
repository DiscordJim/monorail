use std::{collections::{btree_map::{IntoKeys, IntoValues}, BTreeMap}, hash::RandomState};

use crate::core::alloc::{foreign::{transform_foreign, Mono}, Foreign};



pub type MonoBTreeMap<K, V> = Mono<BTreeMap<K, V>>;

impl<K, V> MonoBTreeMap<K, V> {
    #[inline]
    pub const fn new() -> MonoBTreeMap<K, V> {
        Mono::from_const(BTreeMap::new())
    }
    #[inline]
    pub fn into_keys(self) -> Mono<IntoKeys<K, V>> {
        self.project(BTreeMap::into_keys)
    }
    #[inline]
    pub fn into_values(self) -> Mono<IntoValues<K, V>> {
        self.project(BTreeMap::into_values)
    }
}

impl<K, V> Foreign<BTreeMap<K, V>>
where 
    BTreeMap<K, V>: Send
{
    #[inline]
    pub fn into_keys(self) -> Foreign<IntoKeys<K, V>>
    where
        IntoKeys<K, V>: Send
    {
        transform_foreign(self, Mono::<BTreeMap<K, V>>::into_keys)
    }
    #[inline]
    pub fn into_values(self) -> Foreign<IntoValues<K, V>>
    where 
        IntoValues<K, V>: Send
    {
        transform_foreign(self, Mono::<BTreeMap<K, V>>::into_values)
    }
}