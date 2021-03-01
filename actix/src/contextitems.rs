use std::future::Future;
use std::marker::PhantomData;
use std::pin::Pin;
use std::task::{self, Poll};
use std::time::Duration;

use futures_core::{ready, stream::Stream};
use pin_project_lite::pin_project;

use crate::actor::{Actor, ActorContext, AsyncContext};
use crate::clock::Sleep;
use crate::fut::ActorFuture;
use crate::handler::{Handler, Message, MessageResponse};

pub(crate) struct ActorWaitItem<A: Actor>(Pin<Box<dyn ActorFuture<Output = (), Actor = A>>>);

impl<A> ActorWaitItem<A>
where
    A: Actor,
    A::Context: ActorContext + AsyncContext<A>,
{
    #[inline]
    pub fn new<F>(fut: F) -> Self
    where
        F: ActorFuture<Output = (), Actor = A> + 'static,
    {
        ActorWaitItem(Box::pin(fut))
    }

    pub fn poll(
        mut self: Pin<&mut Self>,
        act: &mut A,
        ctx: &mut A::Context,
        task: &mut task::Context<'_>,
    ) -> Poll<()> {
        match self.0.as_mut().poll(act, ctx, task) {
            Poll::Pending => {
                if ctx.state().alive() {
                    Poll::Pending
                } else {
                    Poll::Ready(())
                }
            }
            Poll::Ready(_) => Poll::Ready(()),
        }
    }
}

pin_project! {
    pub(crate) struct ActorDelayedMessageItem<A, M>
    where
        A: Actor,
        M: Message,
    {
        msg: Option<M>,
        #[pin]
        timeout: Sleep,
        act: PhantomData<A>,
    }
}

impl<A, M> ActorDelayedMessageItem<A, M>
where
    A: Actor,
    M: Message,
{
    pub fn new(msg: M, timeout: Duration) -> Self {
        Self {
            msg: Some(msg),
            timeout: actix_rt::time::sleep(timeout),
            act: PhantomData,
        }
    }
}

impl<A, M> ActorFuture for ActorDelayedMessageItem<A, M>
where
    A: Actor + Handler<M>,
    A::Context: AsyncContext<A>,
    M: Message + 'static,
{
    type Output = ();
    type Actor = A;

    fn poll(
        self: Pin<&mut Self>,
        act: &mut A,
        ctx: &mut A::Context,
        task: &mut task::Context<'_>,
    ) -> Poll<Self::Output> {
        let this = self.project();
        ready!(this.timeout.poll(task));
        let fut = A::handle(act, this.msg.take().unwrap(), ctx);
        fut.handle(ctx, None);
        Poll::Ready(())
    }
}

pub(crate) struct ActorMessageItem<A, M>
where
    A: Actor,
    M: Message,
{
    msg: Option<M>,
    act: PhantomData<A>,
}

impl<A: Actor, M: Message> Unpin for ActorMessageItem<A, M> {}

impl<A, M> ActorMessageItem<A, M>
where
    A: Actor,
    M: Message,
{
    pub fn new(msg: M) -> Self {
        Self {
            msg: Some(msg),
            act: PhantomData,
        }
    }
}

impl<A, M> ActorFuture for ActorMessageItem<A, M>
where
    A: Actor + Handler<M>,
    A::Context: AsyncContext<A>,
    M: Message + 'static,
{
    type Output = ();
    type Actor = A;

    fn poll(
        self: Pin<&mut Self>,
        act: &mut A,
        ctx: &mut A::Context,
        _: &mut task::Context<'_>,
    ) -> Poll<Self::Output> {
        let this = self.get_mut();
        let fut = Handler::handle(act, this.msg.take().unwrap(), ctx);
        fut.handle(ctx, None);
        Poll::Ready(())
    }
}

pin_project! {
    pub(crate) struct ActorMessageStreamItem<A, S>
    where
        A: Actor,
    {
        #[pin]
        stream: S,
        act: PhantomData<A>,
    }
}

impl<A, S> ActorMessageStreamItem<A, S>
where
    A: Actor,
{
    pub fn new(st: S) -> Self {
        Self {
            stream: st,
            act: PhantomData,
        }
    }
}

impl<A, M, S> ActorFuture for ActorMessageStreamItem<A, S>
where
    S: Stream<Item = M>,
    A: Actor + Handler<M>,
    A::Context: AsyncContext<A>,
    M: Message + 'static,
{
    type Output = ();
    type Actor = A;

    fn poll(
        self: Pin<&mut Self>,
        act: &mut A,
        ctx: &mut A::Context,
        task: &mut task::Context<'_>,
    ) -> Poll<Self::Output> {
        let mut this = self.project();
        loop {
            match this.stream.as_mut().poll_next(task) {
                Poll::Ready(Some(msg)) => {
                    let fut = Handler::handle(act, msg, ctx);
                    fut.handle(ctx, None);
                    if ctx.waiting() {
                        return Poll::Pending;
                    }
                }
                Poll::Ready(None) => return Poll::Ready(()),
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}
