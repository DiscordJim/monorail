use std::{collections::{hash_map::{IntoKeys, IntoValues}, HashMap}, hash::RandomState};

use crate::core::alloc::{foreign::{transform_foreign, Mono}, Foreign};



pub type MonoHashMap<K, V, S = RandomState> = Mono<HashMap<K, V, S>>;



impl<K, V> MonoHashMap<K, V, RandomState> {
    #[inline]
    pub fn new() -> MonoHashMap<K, V, RandomState> {
        Default::default()
    }
    #[inline]
    pub fn with_capacity(capacity: usize) -> MonoHashMap<K, V, RandomState> {
        Mono::from(HashMap::with_capacity(capacity))
    }
}


impl<K, V, S> MonoHashMap<K, V, S> {
    #[inline]
    pub const fn with_hasher(hash_builder: S) -> MonoHashMap<K, V, S> {
        Mono::from_const(HashMap::with_hasher(hash_builder))
    }
    #[inline]
    pub fn with_capacity_and_hasher(capacity: usize, hasher: S) -> MonoHashMap<K, V, S> {
        Mono::from_const(HashMap::with_capacity_and_hasher(capacity, hasher))
    }
    #[inline]
    pub fn into_keys(self) -> Mono<IntoKeys<K, V>> {
        Mono::from_const(self.into_inner().into_keys())
    }
    #[inline]
    pub fn into_values(self) -> Mono<IntoValues<K, V>> {
        Mono::from_const(self.into_inner().into_values())
    }
}

impl<K, V, S> Foreign<HashMap<K, V, S>>
where 
    HashMap<K, V, S>: Send,
{
    #[inline]
    pub fn into_keys(self) -> Foreign<IntoKeys<K, V>>
    where 
        IntoKeys<K, V>: Send
    {
        transform_foreign(self, |f| f.into_keys())
    }
    #[inline]
    pub fn into_values(self) -> Foreign<IntoValues<K, V>>
    where 
        IntoValues<K, V>: Send
    {
        transform_foreign(self, |f| f.into_values())
    }
}