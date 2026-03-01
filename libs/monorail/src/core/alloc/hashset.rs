// use core::fmt;
// use std::{collections::HashSet, hash::{BuildHasher, Hash, RandomState}, marker::PhantomData, ops::{BitAnd, BitOr, BitXor, Deref, DerefMut, Sub}, rc::Rc};

// use rand::seq::WeightError;

use std::{collections::HashSet, hash::RandomState};

use crate::core::alloc::foreign::Mono;


pub type MonoHashSet<T, S = RandomState> = Mono<HashSet<T, S>>;


impl<T> Mono<HashSet<T, RandomState>> {
    pub fn new() -> Mono<HashSet<T, RandomState>> {
        Mono::default()
    }
    pub fn with_capacity(capacity: usize) -> Mono<HashSet<T, RandomState>> {
        HashSet::with_capacity(capacity).into()
    }
}

impl<T, S> Mono<HashSet<T, S>> {
    pub fn with_capacity_and_hasher(capacity: usize, hasher: S) -> Self {
        Mono::from(HashSet::with_capacity_and_hasher(capacity, hasher))
    }
}


// fn sus() {
//     // let hi = MonoHashSet::<usize>::new().make_foreign();
//     // hi.into
//     // let w = hi.into_iter();
// }

// pub struct MonoHashSet<T, S = RandomState> {
//     inner: HashSet<T, S>,
//     marker: PhantomData<Rc<()>>
// }

// impl<T> MonoHashSet<T, RandomState> {
//     #[inline]
//     pub fn new() -> MonoHashSet<T, RandomState> {
//         Default::default()
//     }

//     #[inline]
//     pub fn with_capacity(capacity: usize) -> MonoHashSet<T, RandomState> {
//         MonoHashSet {
//             inner: HashSet::with_capacity(capacity),
//             marker: PhantomData
//         }
//     }

//     #[inline]
//     pub const fn with_hasher(hasher: S) -> MonoHashSet<T, S> {
//         MonoHashSet {
//             inner: HashSet::with_hasher(hasher),
//             marker: PhantomData
//         }
//     }
// }

// impl<T, S> Deref for MonoHashSet<T, S> {
//     type Target = HashSet<T, S>;
//     fn deref(&self) -> &Self::Target {
//         &self.inner
//     }
// }

// impl<T, S> DerefMut for MonoHashSet<T, S> {
//     fn deref_mut(&mut self) -> &mut Self::Target {
//         &mut self.inner
//     }
// }

// impl<T, S> Clone for MonoHashSet<T, S>
// where 
//     T: Clone,
//     S: Clone
// {
//     #[inline]
//     fn clone(&self) -> Self {
//         Self {
//             inner: self.inner.clone(),
//             marker: PhantomData
//         }
//     }

//     #[inline]
//     fn clone_from(&mut self, source: &Self) {
//         self.inner.clone_from(&source.inner);
//     }
// }

// impl<T, S> PartialEq for MonoHashSet<T, S>
// where 
//     T: Eq + Hash,
//     S: BuildHasher
// {
//     fn eq(&self, other: &Self) -> bool {
//         self.inner.eq(&other.inner)
//     }
// }
// impl<T, S> Eq for MonoHashSet<T, S>
// where 
//     T: Eq + Hash,
//     S: BuildHasher
// {}

// impl<T, S> fmt::Debug for MonoHashSet<T, S>
// where 
//     T: fmt::Debug
// {
//     fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
//         self.inner.fmt(f)
//     }
// }

// impl<T, S> FromIterator<T> for MonoHashSet<T, S>
// where 
//     T: Eq + Hash,
//     S: BuildHasher + Default
// {
//     #[inline]
//     fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
//         Self {
//             inner: HashSet::from_iter(iter),
//             marker: PhantomData
//         }
//     }
// }

// impl<T, const N: usize> From<[T; N]> for MonoHashSet<T, RandomState>
// where 
//     T: Eq + Hash
// {
//     fn from(value: [T; N]) -> Self {
//         Self::from_iter(value)
//     }
// }


// impl<T, S> Default for MonoHashSet<T, S>
// where 
//     S: Default
// {
//     #[inline]
//     fn default() -> Self {
//         Self {
//             inner: HashSet::default(),
//             marker: PhantomData            
//         }
//     }
// }

// impl<T, S> BitOr<&MonoHashSet<T, S>> for &MonoHashSet<T, S>
// where 
//     T: Eq + Hash + Clone,
//     S: BuildHasher + Default
// {
//     type Output = MonoHashSet<T, S>;

//     fn bitor(self, rhs: &MonoHashSet<T, S>) -> Self::Output {
//         MonoHashSet {
//             inner: self.inner.bitor(&rhs.inner),
//             marker: PhantomData
//         }
//     }
// }

// impl<T, S> BitAnd<&MonoHashSet<T, S>> for &MonoHashSet<T, S>
// where
//     T: Eq + Hash + Clone,
//     S: BuildHasher + Default
// {

//     type Output = MonoHashSet<T, S>;
//     fn bitand(self, rhs: &MonoHashSet<T, S>) -> Self::Output {
//         MonoHashSet {
//             inner: self.inner.bitand(&rhs.inner),
//             marker: PhantomData
//         }
//     }

// }

// impl<T, S> BitXor<&MonoHashSet<T, S>> for &MonoHashSet<T, S>
// where 
//     T: Eq + Hash + Clone,
//     S: BuildHasher + Default
// {
//     type Output = MonoHashSet<T, S>;
//     fn bitxor(self, rhs: &MonoHashSet<T, S>) -> Self::Output {
//         MonoHashSet {
//             inner: self.inner.bitxor(&rhs.inner),
//             marker: PhantomData
//         }
//     }
// }


// impl<T, S> Sub<&MonoHashSet<T, S>> for &MonoHashSet<T, S>
// where 
//     T: Eq + Hash + Clone,
//     S: BuildHasher + Default
// {
//     type Output = MonoHashSet<T, S>;
//     fn sub(self, rhs: &MonoHashSet<T, S>) -> Self::Output {
//         MonoHashSet {
//             inner: self.inner.sub(&rhs.inner),
//             marker: PhantomData
//         }
//     }
// }

// impl<T, S> IntoIterator for MonoHashSet<T, S> {
//     type Item = T;
//     type IntoIter = IntoIterator<T>;

//     fn into_iter(self) -> Self::IntoIter {
        
//     }
// }