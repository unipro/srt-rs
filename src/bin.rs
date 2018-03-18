extern crate srt;

#[macro_use]
extern crate futures;

extern crate log;
extern crate simple_logger;
extern crate tokio;

use log::LevelFilter;

use std::net::ToSocketAddrs;
use srt::socket::SrtSocketBuilder;
use srt::connection::Connection;
use srt::receiver::Receiver;
use std::net::UdpSocket;

use futures::prelude::*;

use tokio::executor::current_thread;

struct Printer {
    recvr: Receiver,
    out_sock: UdpSocket,
}

impl Future for Printer {
    type Item = ();
    type Error = ();

    fn poll(&mut self) -> Poll<(), ()> {
        loop {
            let buf = match self.recvr.poll() {
                Ok(Async::Ready(b)) => b,
                Ok(Async::NotReady) => return Ok(Async::NotReady),
                e => {
                    eprintln!("Error: {:?}", e);
                    continue;
                }
            };

            if let Some(mut b) = buf {
                self.out_sock.send_to(b.as_mut(), "127.0.0.1:1888").unwrap();
            } else {
                println!("Shutdown!");

                return Ok(Async::Ready(()));
            }
        }
    }
}

fn main() {
    simple_logger::init().unwrap();
    log::set_max_level(LevelFilter::Info);

    let pending_connection = SrtSocketBuilder::new(
        "127.0.0.1:1231".to_socket_addrs().unwrap().next().unwrap(),
    ).build()
        .unwrap();

    current_thread::run(|_| {
        current_thread::spawn(
            pending_connection
                .map_err(|e| eprintln!("Error: {:?}", e))
                .and_then(|c| match c {
                    Connection::Recv(r) => Printer {
                        recvr: r,
                        out_sock: UdpSocket::bind("127.0.0.1:0").unwrap(),
                    },
                    Connection::Send(_) => unimplemented!(),
                }),
        );
    });
}
