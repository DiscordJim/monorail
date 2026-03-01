use std::{future::Future, pin::Pin};

use crate::core::actor::base::{Actor, SelfAddr};

pub type BoxedCall<A> =
    Box<
        dyn for<'a> FnOnce(
            SelfAddr<'a, A>,
            &'a mut <A as Actor>::State,
        ) -> Pin<Box<dyn Future<Output = ()> + 'a>>
        + 'static
    >;