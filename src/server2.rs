use dns;
use futures;
use futures_mio;
use std::net;
use errors;

use std::rc::Rc;
use std::cell::RefCell;
use std::io;
use std::net::{SocketAddr};
use std::str;
use futures::{Future, Task, Poll};
use futures::stream::{Stream};

#[derive (Debug, Clone)]
struct MessageData {
    message: dns::Message,
    bytes: Vec<u8>,
    sender_address: SocketAddr
}

#[derive (Debug)]
struct DatagramListener {
    sock: futures_mio::UdpSocket,
    done: bool
}

impl DatagramListener {
    fn new( sock: futures_mio::UdpSocket) -> DatagramListener {
        DatagramListener {
            sock: sock,
            done: false
        }
    }
}

const MAX_UDP_PACKET_SIZE: usize = 2048;

impl Stream for DatagramListener {
    type Item = MessageData;
    type Error = errors::Error;

    fn poll(&mut self, _: &mut Task) -> futures::Poll<Option<MessageData>, errors::Error> {
        let mut buf = [0u8;512];
        match self.sock.recv_from(&mut buf) {
            Ok((0, _)) => (),
            Ok((n, a)) => {
                println!("read {} bytes from {}.", n, a);
                let msg = match dns::Message::new(&buf[..n]) {
                    Ok(m) => m,
                    Err(e) => return futures::Poll::Err(errors::Error::DnsParsingError(e))
                };
                let msg = MessageData {
                    message: msg,
                    bytes: buf.to_vec(),
                    sender_address: a
                };
                return futures::Poll::Ok(Some(msg))
            },
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                return futures::Poll::NotReady
            }
            Err(e) => return futures::Poll::Err(errors::Error::Io(e)),
        }

        futures::Poll::NotReady
    }

    fn schedule(&mut self, task: &mut futures::Task) {
        if !self.done {
            self.sock.schedule(task)
        }
        else {
            task.notify()
        }
    }
}

#[derive (Debug)]
enum UpstreamRequestState {
    Sending,
    Receiving,
    Done,
}

#[derive (Debug)]
struct UpstreamRequest {
    socket: Rc<RefCell<futures_mio::UdpSocket>>,
    request: MessageData,
    remote: net::SocketAddr,
    state: UpstreamRequestState
}

impl Future for UpstreamRequest {
    type Item = MessageData;
    type Error = errors::Error;

    fn poll(&mut self, task: &mut Task) -> futures::Poll<MessageData, errors::Error> {
        use self::UpstreamRequestState::*;
        match self.state {
            Sending => {
                println!("in sending state!");
                match self.socket.borrow_mut().send_to(&self.request.bytes, &self.remote) {
                    Ok(0) => {
                        println!("read 0 bytes?");
                        ()
                    },
                    Ok(n) => {
                        println!("sent {} bytes", n);
                        self.state = UpstreamRequestState::Receiving;
                        return Poll::NotReady
                    }
                    Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                        return Poll::NotReady
                    }
                    Err(e) => {
                        return futures::Poll::Err(errors::Error::Io(e))
                    }
                }
            },
            Receiving => {
                let mut buf = [0u8;512];
                match self.socket.borrow_mut().recv_from(&mut buf) {
                    Ok((0, _)) => (),
                    Ok((n, a)) => {
                        println!("read {} bytes from {}.", n, a);
                        let msg = match dns::Message::new(&buf[..n]) {
                            Ok(m) => m,
                            Err(e) => return futures::Poll::Err(errors::Error::DnsParsingError(e))
                        };
                        self.state = UpstreamRequestState::Done;
                        let msg = MessageData{
                            message: msg,
                            bytes: buf[..n].to_vec(),
                            sender_address: a
                        };
                        return futures::Poll::Ok(msg)
                    },
                    Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                        return futures::Poll::NotReady
                    }
                    Err(e) => return futures::Poll::Err(errors::Error::Io(e)),
                }
            },
            UpstreamRequestState::Done => {
                return futures::Poll::NotReady
            }
        }
        Poll::NotReady
    }

    fn schedule(&mut self, task: &mut futures::Task) {
        match self.state {
            UpstreamRequestState::Done => task.notify(),
            _ => self.socket.borrow_mut().schedule(task),
        };
    }
}

pub fn serve(addr: &net::SocketAddr) {
    let mut l = futures_mio::Loop::new().unwrap();

    let server = l.handle().udp_bind(addr);
    let socket = l.run(server).unwrap();
    println!("Bound UDP Socket: {:?}", socket);

    let outbound = l.handle().udp_bind(&"0.0.0.0:0".parse().unwrap());
    let outbound = Rc::new(RefCell::new(l.run(outbound).unwrap()));

    let upstream_addr: SocketAddr = "8.8.8.8:53".parse().unwrap();

    let dg = DatagramListener::new(socket)
        .and_then(move |msg| {
            let u = UpstreamRequest{
                state: UpstreamRequestState::Sending,
                socket: outbound.clone(),
                request: msg,
                remote: upstream_addr.clone()
            };
            u
        })
        .for_each(|msg| {
            println!("got a {:?}", msg);
            Ok(())
        });

    let x = l.run(dg).unwrap();
    println!("x = {:?}", x);
}
