// pub trait MakeForeign {
//     fn make_foreign(self) -> Foreign
// }

use std::{borrow::{Borrow, Cow}, collections::{btree_map, hash_map::{self, IntoValues}, HashMap, HashSet}, error::Error, fmt, future::Future, hash::Hash, iter::FusedIterator, marker::PhantomData, ops::{BitAnd, BitAndAssign, BitOr, BitOrAssign, BitXor, Deref, DerefMut, Index, IndexMut}, path::Iter, pin::Pin, rc::Rc, slice::SliceIndex, vec::IntoIter};

use crate::{
    core::{alloc::{monobox::MonoBox, monocow::MonoCow, monohm::MonoHashMap, monovec::MonoVec}, shard::state::ShardId},
    monolib::{self, shard_id},
};

mod sealed {
    pub trait Seal {}

    
}

impl<T> sealed::Seal for MonoBox<T> {}
impl<T> sealed::Seal for MonoVec<T> {}
impl<T> sealed::Seal for IntoIter<T> {}
impl<K, V, S> sealed::Seal for MonoHashMap<K, V, S> {}
impl<K, V> sealed::Seal for hash_map::IntoValues<K, V> {}
impl<K, V> sealed::Seal for hash_map::IntoKeys<K, V> {}
impl<K, V> sealed::Seal for hash_map::IntoIter<K, V> {}
impl<'a, B: ?Sized + ToOwned> sealed::Seal for MonoCow<'a, B> {}
// impl<K, V> sealed::Seal for btree_map::

// pub struct UnSend(*const ());
// unsafe impl Sync for UnSend {}

// #[pin_project::pin_project]
#[repr(transparent)]
pub struct Mono<T> {
    internal: T,
    _marker: PhantomData<Rc<()>>,
    // _tag: PhantomData<M>
}

impl<T, O> Mono<Result<T, O>> {
    pub(crate) fn invert_result(self) -> Result<Mono<T>, O> {
        match self.internal {
            Ok(v) => Ok(Mono::from_const(v)),
            Err(e) => Err(e)
        }
    }
}

impl<T> Mono<T> {
    #[inline]
    pub fn project<F, R>(self, f: F) -> Mono<R>
    where 
        F: FnOnce(T) -> R
    {
        Mono {
            internal: f(self.internal),
            _marker: PhantomData
        }
    }
}

// unsafe impl<T: 'static> Send for Mono<T, Forn> {} 


// impl<T> Mono<T, Local>
// where 
//     T: SendForeign
// {
//     pub fn make_foreign(x: T) -> Mono<T, Forn> {
//         Mono {
//             internal: x,
//             _marker: PhantomData,
//             _tag: PhantomData   
//         }
//     }
// }

// impl<T

// impl<T> From<Mono<T, Local>> for Mono<T, Forn>
// where 
//     T: Send
// {

// }




// impl<'a, T> Iterator for &'a Mono<T>
// where 
//     &'a T: Iterator
// {
//     type Item = <&'a T as Iterator>::Item;
//     fn next(&mut self) -> Option<Self::Item> {
//         (&self.internal).next()
//     }
// }



pub struct Local;
pub struct Forn;

impl<T> Hash for Mono<T>
where 
    T: Hash
{
    #[inline]
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.internal.hash(state);
    }
}

impl<T, I> Index<I> for Mono<T>
where 
    T: Index<I>
{
    type Output = <T as Index<I>>::Output;
    fn index(&self, index: I) -> &Self::Output {
        self.internal.index(index)
    }
}

// impl

// impl<A, B> PartialOrd<Mono<B>> for Mono<A>
// where 
//     A: PartialOrd<B>
// {
//     #[inline]
//     fn partial_cmp(&self, other: &Mono<B>) -> Option<std::cmp::Ordering> {
//         self.internal.partial_cmp(&other.internal)
//     }
// }

impl<T, I> IndexMut<I> for Mono<T>
where 
    T: IndexMut<I>
{
    fn index_mut(&mut self, index: I) -> &mut Self::Output {
        self.internal.index_mut(index)
    }
}

// i

// impl<T, I: 

impl<E: Error> Error for Mono<E> {
    #[allow(deprecated)]
    fn cause(&self) -> Option<&dyn Error> {
        Error::cause(&self.internal)
    }
    #[allow(deprecated)]
    fn description(&self) -> &str {
        Error::description(&self.internal)
    }
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Error::source(&self.internal)
    }
    
}

impl<T> Mono<T> {
    #[inline]
    pub const fn from_const(x: T) -> Self {
        Self { internal: x, _marker: PhantomData }
    }
    #[inline]
    pub fn into_inner(self) -> T {
        self.internal
    }
}

// impl<T> Mono<T> {
//     pub fn new(x: T) -> Self {
//         Self {
//             internal: x,
//             _marker: PhantomData
//         }
//     }
// }

impl<T> Deref for Mono<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        &self.internal
    }
}

impl<T> DerefMut for Mono<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.internal
    }
}

impl<T, A> FromIterator<A> for Mono<T>
where 
    T: FromIterator<A>
{
    fn from_iter<I: IntoIterator<Item = A>>(iter: I) -> Self {
        Self::from_const(T::from_iter(iter))
    }
}

// impl<T, A> From<A> for Mono<T>
// where 
//     T: From<A>
// {
//     fn from(value: A) -> Self {
//         Self {
//             internal: T::from(value),
//             _marker: PhantomData
//         }
//     }
// }

impl<T> Mono<T> {
    #[inline]
    pub fn from_inner<A>(value: A) -> Mono<T>
    where 
        T: From<A>
    {
        Mono::from(T::from(value))
    }
}

impl<'a, T> BitOr<&'a Mono<T>> for &'a Mono<T>
where 
    &'a T: BitOr<&'a T>
{
    type Output = Mono<<&'a T as BitOr<&'a T>>::Output>;

    fn bitor(self, rhs: &'a Mono<T>) -> Self::Output {
        Mono::from(self.internal.bitor(&rhs.internal))
    }
}

impl<'a, T> BitAnd<&'a Mono<T>> for &'a Mono<T>
where 
    &'a T: BitAnd<&'a T>
{
    type Output = Mono<<&'a T as BitAnd<&'a T>>::Output>;
    fn bitand(self, rhs: &'a Mono<T>) -> Self::Output {
        Mono::from(self.internal.bitand(&rhs.internal))
    }
}

impl<'a, T> BitXor<&'a Mono<T>> for &'a Mono<T>
where 
    &'a T: BitXor<&'a T>
{
     type Output = Mono<<&'a T as BitXor<&'a T>>::Output>;
     fn bitxor(self, rhs: &'a Mono<T>) -> Self::Output {
         Mono::from(self.internal.bitxor(&rhs.internal))
     }
}



// impl<T> AsRef<Mono<&T>> for Mono<T> {
//     fn as_ref(&self) -> &Mono<&T> {
    
//     }
// }

impl<A, B> BitOr<Mono<B>> for Mono<A>
where 
    A: BitOr<B>
{
    type Output = Mono<<A as BitOr<B>>::Output>;
    fn bitor(self, rhs: Mono<B>) -> Self::Output {
        Mono::from(self.internal.bitor(rhs.internal))
    }
}

impl<A, B> BitAnd<Mono<B>> for Mono<A>
where 
    A: BitAnd<B>
{
    type Output = Mono<<A as BitAnd<B>>::Output>;
    fn bitand(self, rhs: Mono<B>) -> Self::Output {
        Mono::from(self.internal.bitand(rhs.internal))
    }
}

impl<A, B> BitXor<Mono<B>> for Mono<A>
where 
    A: BitXor<B>
{
    type Output = Mono<<A as BitXor<B>>::Output>;
    fn bitxor(self, rhs: Mono<B>) -> Self::Output {
        Mono::from(self.internal.bitxor(rhs.internal))
    }
}

impl<A, B> BitAndAssign<Mono<B>> for Mono<A>
where 
    A: BitAndAssign<B>
{
    fn bitand_assign(&mut self, rhs: Mono<B>) {
        self.internal.bitand_assign(rhs.internal);
    }
}

// impl<A.

// impl<A, B> BitAnd<Mono<B>> for Mono<A>
// where 
//     A: BitAnd<Mono<B>>
// {
//     type Output = Mono<<A as BitOr<B>::Output>>;
//     fn bitand(self, rhs: Mono<B>) -> Self::Output {
//         Mono::from(self.internal.bitand(rhs))
//     }
// }

// impl<T> BitAnd for Mono<T>
// where 
//     T: BitAnd<T>
// {
//     // type Output = Mono<<T as BitAnd<O>>;
// }

impl<T> Clone for Mono<T>
where 
    T: Clone
{
    #[inline]
    fn clone(&self) -> Self {
        Self::from_const(self.internal.clone())
    }

    #[inline]
    fn clone_from(&mut self, source: &Self) {
        self.internal.clone_from(&source.internal);
    }
}

impl<T> From<T> for Mono<T> {
    #[inline]
    fn from(value: T) -> Self {
        Mono::from_const(value)
    }
}

impl<T: Default> Default for Mono<T> {

    #[inline]
    fn default() -> Self {
        Self::from_const(T::default())
    }
}

// impl<T> FusedIterator for Mono<T>
// where 
//     T: FusedIterator
// {}

impl<T> PartialEq for Mono<T>
where 
    T: PartialEq
{
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        self.internal.eq(&other.internal)
    }
    #[inline]
    fn ne(&self, other: &Self) -> bool {
        self.internal.ne(&other.internal)
    }
}

impl<T> fmt::Debug for Mono<T>
where 
    T: fmt::Debug
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.internal.fmt(f)
    }
}

impl<T> fmt::Display for Mono<T>
where 
    T: fmt::Display
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.internal.fmt(f)
    }
}


impl<T> Eq for Mono<T>
where 
    T: Eq
{}

// impl<T> Iterator for Mono<T>
// where 
//     T: Iterator
// {
//     type Item = Mono<T::Item>;
//     fn next(&mut self) -> Option<Self::Item> {
//         Some(Mono {
//             internal: self.internal.next()?,
//             _marker: PhantomData
//         })
//     }
// }

pub struct MonoIterator<T>(Mono<T>);

impl<T: Iterator> Iterator for MonoIterator<T> {
    type Item = T::Item;
    fn next(&mut self) -> Option<Self::Item> {
        self.0.internal.next()
    }   
}

// impl<T, A> IntoIterator<A> for Mono<T>
// where 
//     T: IntoIterator<A>
// {
//     type IntoIter = M;
// }

impl<T> IntoIterator for Mono<T>
where 
    T: IntoIterator
{
    // type IntoIter = Into;
    type IntoIter = MonoIterator<<T as IntoIterator>::IntoIter>;
    type Item = <T as IntoIterator>::Item;

    fn into_iter(self) -> Self::IntoIter {
        let t: <T as IntoIterator>::IntoIter = self.internal.into_iter();
        MonoIterator(Mono::from(t))
    }
}

impl<T> DoubleEndedIterator for MonoIterator<T>
where 
    T: DoubleEndedIterator
{
    #[inline]
    fn next_back(&mut self) -> Option<Self::Item> {
        self.0.internal.next_back()
    }

}

impl<T> FusedIterator for MonoIterator<T>
where 
    T: FusedIterator
{}


pub unsafe trait SendForeign {}


// unsafe impl<T: SendForeign> Send for T {}

// // unsafe impl 
// unsafe impl<T: Send> SendForeign for T {}


// // unsafe impl<T: Send> Send for Foreign<T> {}
// unsafe impl<T: SendForeign + 'static> Send for Foreign<T> {}

// unsafe impl<T> SendForeign for MonoBox<T>
// where 
//     T: ?Sized + Send {}


impl<T> Mono<T>
where 
    Mono<T>: SendForeign
{
    pub fn make_foreign(self) -> Foreign<T> {
        Foreign {
            object: Some(Mono::from(self.internal)),
            origin: shard_id()
        }
    }
}

// impl

impl<T> From<T> for Foreign<T>
where 
    T: Send
{
    fn from(value: T) -> Self {
        Mono::from_const(value).make_foreign()
    }
}

unsafe impl<T: 'static> Send for Foreign<T> {}


impl<T> Foreign<T>
where 
    T: Send
{
    pub fn new(value: T) -> Self {
        Self {
            object: Some(Mono::from_const(value)),
            origin: shard_id()
        }
    }
}

unsafe impl<T> SendForeign for Mono<T>
where 
    T: Send
{}

// unsafe impl<T> SendForeign for Mono<Box<T>>
// where 
//     T: ?Sized,
//     Box<T>: Send
// {}

// unsafe impl<T> SendForeign for Mono<Vec<T>>
// where 
//     Vec<T>: Send
// {}

// unsafe impl<K, V, S> SendForeign for Mono<HashMap<K, V, S>>
// where 
//     HashMap<K, V, S>: Send
// {}

// unsafe impl<T, S> SendForeign for Mono<HashSet<T, S>>
// where 
//     HashSet<T, S>: Send
// {}

// unsafe impl<'a, B> SendForeign for Mono<Cow<'a, B>>
// where
//     B: ?Sized + ToOwned,
//     Cow<'a, B>: Send
// {}


// unsafe impl<T> SendForeign for MonoBox<T>
// where 
//     T: ?Sized,
//     Box<T>: Send
// {}

// unsafe impl<T> SendForeign for MonoVec<T>
// where 
//     Vec<T>: Send
// {}

// unsafe impl<K, V, S> SendForeign for MonoHashMap<K, V, S>
// where 
//     HashMap<K, V, S>: Send
// {}

// // unsafe impl<K, S> SendForeign for MonoHashSet<K, S>
// // where 
// //     HashSet<K, S>: Send
// // {}

// unsafe impl<'a, B> SendForeign for MonoCow<'a, B>
// where 
//     B: ?Sized + ToOwned,
//     Cow<'a, B>: Send
// {}





// unsafe impl<T: Send + ?Sized> Send for Foreign<MonoBox<T>> {}
// unsafe impl<T: Send> Send for Foreign<MonoVec<T>> {}
// unsafe impl<T: Send> Send for Foreign<IntoIter<T>> {}
// unsafe impl<K, V, S> Send for Foreign<MonoHashMap<K, V, S>>
// where 
//     HashMap<K, V, S>: Send {}

// unsafe impl<K, V> Send for Foreign<hash_map::IntoKeys<K, V>>
// where 
//     hash_map::IntoKeys<K, V>: Send {}

// unsafe impl<K, V> Send for Foreign<hash_map::IntoValues<K, V>>
// where 
//     hash_map::IntoValues<K, V>: Send {}

// unsafe impl<K, V> Send for Foreign<hash_map::IntoIter<K, V>>
// where 
//     hash_map::IntoIter<K, V>: Send {}

// unsafe impl<'a, B> Send for Foreign<MonoCow<'a, B>>
// where 
//     B: ?Sized + ToOwned,
//     Cow<'a, B>: Send {}






// unsafe impl<T> Send for ForeignUnique<T> where T: MakeForeign + 'static {}


#[inline]
pub(crate) fn transform_foreign<T, U>(mut ptr: Foreign<T>, f: impl FnOnce(Mono<T>) -> Mono<U>) -> Foreign<U>
where 
    T: Send,
    U: Send
{
    Foreign {
       object: ptr.object.take().map(f),
       origin: ptr.origin
 
    }
}


pub struct Foreign<T>
where
    Self: Send + 'static,
{
    object: Option<Mono<T>>,
    origin: ShardId
}

impl<T> Foreign<T>
where 
    Self: Send + 'static
{
    // #[inline]
    // pub(crate) fn into_inner(self) -> T {
    //     let man =
    // }
    #[inline]
    pub(crate) fn project<F, R>(mut self, f: F) -> Foreign<R>
    where 
        F: FnOnce(T) -> R,
        Foreign<R>: Send + 'static,
        T: Send,
        R: Send
    {
        let a = Foreign {
            object: self.object.take().map(|mo| mo.project(f)),
            origin: self.origin
        };

        a
    }
    #[inline]
    pub(crate) fn try_project<F, R, E>(mut self, f: F) -> Result<Foreign<R>, Foreign<E>>
    where 
        F: FnOnce(T) -> Result<R, E>,
        Foreign<R>: Send + 'static,
        T: Send,
        R: Send
    {

        let origin = self.origin;
        match self.object.take().map(|mo| mo.project(f)) {
            Some(v) => {
                match v.into_inner() {
                    Ok(good) => Ok(Foreign {
                        object: Some(Mono::from_const(good)),
                        origin
                    }),
                    Err(bad) => Err(Foreign {
                        object: Some(Mono::from_const(bad)),
                        origin
                    })
                }
                // v.into_inner().map(|e| Foreign { object: Some(Mono::from_const(e)), origin })
            }
            None => panic!("Empty foreign.")
        }
    }
}

impl<T, O> Foreign<Result<T, O>> {
    pub(crate) fn invert_result(mut self) -> Result<Foreign<T>, O> {
        let origin = self.origin;
        match self.object.take().unwrap().into_inner() {
            Ok(v) => Ok(Foreign {
                object: Some(Mono::from_const(v)),
                origin: origin
            }),
            Err(e) => Err(e)
        }
    }
}

// pub fn 

// impl<T> Future for Mono<T>
// where 
//     T: Future + Unpin,
//     // Foreign<T>: Send + 'static
// {
//     type Output = T::Output;

//     fn poll(mut self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<Self::Output> {
//         Pin::new(self.internal).poll(cx)
//     }
// }

impl<T> Future for Foreign<T>
where 
    T: Future + Unpin,
    Mono<T>: Unpin,
    Foreign<T>: Send + 'static
{
    type Output = T::Output;

    fn poll(mut self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<Self::Output> {
        Pin::new(&mut **self.object.as_mut().unwrap()).poll(cx)
    }
}

impl<I> Iterator for Foreign<I>
where 
    I: Iterator,
    Foreign<I>: Send + 'static
{
    type Item = I::Item;

    fn next(&mut self) -> Option<Self::Item> {
        I::next(self.object.as_mut().unwrap())
    }

}




// pub(crate) fn extract_foreign_ptr<T>(
//     mut frn: Foreign<T>
// ) -> Mono<T>
// where 
//     Foreign<T>: Send + 'static
// {
//     frn.object.take().unwrap()
// }

pub(crate) fn create_foreign_ptr<T>(
    object: T
) -> Foreign<T>
where 
    Foreign<T>: Send + 'static
{
    Foreign { 
        object: Some(Mono::from_const(object)),
        origin: shard_id()
     }
}

// impl<T> Foreign<T>
// where 
//     T: MakeForeign
// {
//     pub(crate) fn new(x: T) -> Foreign<T> {
//         Foreign {
//             object: ForeignUnique {
//                 object: Some(x),
//                 origin: shard_id()
//             }
//         }
//     }
// }




impl<T> Deref for Foreign<T>
where
    Self: Send + 'static,
{
    type Target = Mono<T>;
    fn deref(&self) -> &Self::Target {
        self.object.as_ref().unwrap()
    }
}

impl<T> DerefMut for Foreign<T>
where
    Self: Send + 'static,
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.object.as_mut().unwrap()
    }
}

impl<T> Drop for Foreign<T>
where
    Self: Send + 'static,
{
    fn drop(&mut self) {
        if std::mem::needs_drop::<T>() {
            // This can be somewhat expensive, so not worth invoking if
            // we do not even need a drop handler.
            if shard_id() != self.origin {
                let inner = Foreign {
                    object: self.object.take(),
                    origin: self.origin
                };
                monolib::submit_to(self.origin, async move || {
                    drop(inner);
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{mem::ManuallyDrop, sync::atomic::{AtomicBool, AtomicIsize, AtomicUsize, Ordering}, time::Duration};

    use crate::{core::{alloc::{monobox::MonoBox}, shard::{shard::signal_monorail, state::ShardId}, topology::{MonorailConfiguration, MonorailTopology}}, monolib::{shard_id, submit_to}};




    #[test]
    pub fn test_foreign_return() {
        static DROP_CHECK: AtomicIsize = AtomicIsize::new(-1);

        struct CheckedDrop {
            
        }

        impl Drop for CheckedDrop {
            fn drop(&mut self) {
                DROP_CHECK.store(shard_id().as_usize() as isize, Ordering::Release);
                signal_monorail(Ok(()));
            }
        }

        MonorailTopology::setup(MonorailConfiguration::builder().with_core_override(3).build(), async || {

            submit_to(ShardId::new(1), async || {
                let checked = MonoBox::new(CheckedDrop {}).make_foreign();

                submit_to(ShardId::new(2), async move || {
                    drop(checked);
                });
            });
            

        }).unwrap();

        assert_eq!(DROP_CHECK.load(Ordering::Acquire) as usize, 1);

    }

    #[test]
    pub fn test_foreign_clone_on_different_core() {
        // TEST: If you clone a foreign on another core it should
        // rebind to the new core.
        static DROP_CHECK: AtomicIsize = AtomicIsize::new(-1);
        static DROP_COUNT: AtomicIsize = AtomicIsize::new(0);


        #[derive(Clone)]
        struct CheckedDrop {
            name: &'static str
        }

        impl Drop for CheckedDrop {
            fn drop(&mut self) {
                println!("Dropping {} on {:?}", self.name, shard_id());
                DROP_CHECK.store(shard_id().as_usize() as isize, Ordering::Release);
                if DROP_COUNT.fetch_add(1, Ordering::AcqRel) == 1 {
                    signal_monorail(Ok(()));
                }
                // signal_monorail(Ok(()));
            }
        }

        MonorailTopology::setup(MonorailConfiguration::builder().with_core_override(3).build(), async || {

            submit_to(ShardId::new(1), async || {
                let checked = MonoBox::new(CheckedDrop {
                    name: "first"
                }).make_foreign();

                submit_to(ShardId::new(2), async move || {
                    let mut other = checked.clone();
                    other.name = "second";

                    println!("Running on core: {:?}", shard_id());

                    // let other = ManuallyDrop::

                    let mut other = ManuallyDrop::new(other);
                    let mut checked = ManuallyDrop::new(checked);

                    unsafe {
                        ManuallyDrop::drop(&mut checked);
                        std::thread::sleep(Duration::from_millis(10));
                        ManuallyDrop::drop(&mut other);
                    }

                    // drop(checked);
                    // drop(other);
                });
            });
            

        }).unwrap();

        assert_eq!(DROP_CHECK.load(Ordering::Acquire) as usize, 2);

    }
    

    #[test]
    pub fn test_special_drop() {
        // println!("HEK: {:?}",)
    }
}
