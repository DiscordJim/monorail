use std::{future::Future, marker::PhantomData, task::Poll};



pub(crate) fn select2<F1, F2, O1, O2>(fut1: F1, fut2: F2) -> Select2Fut<F1, F2, O1, O2>
where 
    F1: Future<Output = O1>,
    F2: Future<Output = O2>
{
    Select2Fut { a: fut1, b: fut2, _marker: PhantomData }

}
#[pin_project::pin_project]
pub(crate) struct Select2Fut<F1, F2, O1, O2> {
    #[pin]
    a: F1,
    #[pin]
    b: F2,
    _marker: PhantomData<(O1, O2)>
}

#[derive(PartialEq, Eq, Debug)]
pub enum Select2Result<A, B> {
    Left(A),
    Right(B)
}

impl<F1, F2, O1, O2> Future for Select2Fut<F1, F2, O1, O2> 
where 
    F1: Future<Output = O1>,
    F2: Future<Output = O2>
{
    type Output = Select2Result<O1, O2>;
    fn poll(self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<Self::Output> {
        let this = self.project();

        if let Poll::Ready(t) = this.a.poll(cx) {
            return Poll::Ready(Select2Result::Left(t));
        }
        if let Poll::Ready(t) = this.b.poll(cx) {
            return Poll::Ready(Select2Result::Right(t));
        }
        Poll::Pending
    }
}


#[cfg(test)]
mod tests {
    use std::{future::{pending, ready, Future}, task::{Context, Poll, Waker}};

    use crate::core::executor::helper::{select2, Select2Result};


    #[test]
    pub fn test_sel2() {

        // struct TestWaker {}

        // impl 

        let waker = Waker::noop();
        let mut ctx = Context::from_waker(waker);

        let multi = select2(ready(32), pending::<i32>());
        assert_eq!(core::pin::pin!(multi).poll(&mut ctx), Poll::Ready(Select2Result::Left(32)));
        
         let multi = select2(pending::<i32>(), ready::<i32>(32));
        assert_eq!(core::pin::pin!(multi).poll(&mut ctx), Poll::Ready(Select2Result::Right(32)));
        


    }
}