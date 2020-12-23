#![allow(clippy::let_unit_value)]
use std::net;
use std::pin::Pin;
use std::str::FromStr;
use std::task::{Context as StdContext, Poll};

use actix::prelude::*;
use tokio::net::{TcpListener, TcpStream};
use tokio_util::codec::FramedRead;

mod codec;
mod server;
mod session;

use codec::ChatCodec;
use server::ChatServer;
use session::ChatSession;

/// Define TCP server that will accept incoming TCP connection and create
/// chat actors.
struct Server {
    chat: Addr<ChatServer>,
}

/// Make actor from `Server`
impl Actor for Server {
    /// Every actor has to provide execution `Context` in which it can run.
    type Context = Context<Self>;
}

#[derive(Message)]
#[rtype(result = "()")]
struct TcpConnect(pub TcpStream, pub net::SocketAddr);

/// Handle stream of TcpStream's
impl Handler<TcpConnect> for Server {
    /// this is response for message, which is defined by `ResponseType` trait
    /// in this case we just return unit.
    type Result = ();

    fn handle(&mut self, msg: TcpConnect, _: &mut Context<Self>) {
        // For each incoming connection we create `ChatSession` actor
        // with out chat server address.
        let server = self.chat.clone();
        ChatSession::create(move |ctx| {
            let (r, w) = tokio::io::split(msg.0);
            ChatSession::add_stream(FramedRead::new(r, ChatCodec), ctx);
            ChatSession::new(server, actix::io::FramedWrite::new(w, ChatCodec, ctx))
        });
    }
}

#[actix::main]
async fn main() {
    // Start chat server actor
    let server = ChatServer::default().start();

    // Create server listener
    let addr = net::SocketAddr::from_str("127.0.0.1:12345").unwrap();
    let listener = TcpListener::bind(&addr).await.unwrap();

    struct WtfStream {
        listener: TcpListener,
    }

    impl Stream for WtfStream {
        type Item = TcpConnect;

        fn poll_next(
            self: Pin<&mut Self>,
            cx: &mut StdContext<'_>,
        ) -> Poll<Option<Self::Item>> {
            match self.get_mut().listener.poll_accept(cx) {
                Poll::Ready(Ok((st, addr))) => Poll::Ready(Some(TcpConnect(st, addr))),
                Poll::Ready(Err(e)) => {
                    if e.kind() == std::io::ErrorKind::WouldBlock {
                        Poll::Pending
                    } else {
                        Poll::Ready(None)
                    }
                }
                Poll::Pending => Poll::Pending,
            }
        }
    }

    // Our chat server `Server` is an actor, first we need to start it
    // and then add stream on incoming tcp connections to it.
    // TcpListener::incoming() returns stream of the (TcpStream, net::SocketAddr)
    // items So to be able to handle this events `Server` actor has to implement
    // stream handler `StreamHandler<(TcpStream, net::SocketAddr), io::Error>`
    Server::create(move |ctx| {
        ctx.add_message_stream(WtfStream { listener });
        Server { chat: server }
    });

    println!("Running chat server on 127.0.0.1:12345");

    tokio::signal::ctrl_c().await.unwrap();
    println!("Ctrl-C received, shutting down");
    System::current().stop();
}
