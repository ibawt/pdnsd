use dns;
use futures;
use futures_mio::{self, UdpSocket, LoopHandle};
use std::net;
use errors;

use std::rc::Rc;
use std::cell::RefCell;
use std::io;
use std::net::{SocketAddr};
use std::str;
use futures::{Future, Poll};
use futures::stream::{Stream};

#[derive (Debug, Clone)]
struct MessageData {
    message: dns::Message,
    bytes: Vec<u8>,
    sender_address: SocketAddr
}


#[derive (Debug)]
struct SendDatagram {
    socket: Rc<RefCell<UdpSocket>>,
    bytes: Vec<u8>,
    address: SocketAddr,
    done: bool
}

impl SendDatagram {
    fn new(socket: Rc<RefCell<UdpSocket>>, bytes: Vec<u8>, address: SocketAddr) -> SendDatagram {
        SendDatagram{
            socket: socket,
            bytes: bytes,
            address: address,
            done: false
        }
    }
}

impl Future for SendDatagram {
    type Item = Rc<RefCell<UdpSocket>>;
    type Error = errors::Error;

    fn poll(&mut self) -> Poll<Rc<RefCell<UdpSocket>>, errors::Error> {
        match self.socket.borrow_mut().send_to(&self.bytes, &self.address) {
            Ok(0) => Poll::NotReady,
            Ok(n) => {
                if n != self.bytes.len() {
                    println!("not sending");
                }
                println!("sent {} bytes to {}", n, self.address);
                self.done = true;
                Poll::Ok(self.socket.clone())
            }
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                Poll::NotReady
            }
            Err(e) => {
                Poll::Err(errors::Error::Io(e))
            }
        }
    }
}

#[derive (Debug)]
struct ReceiveDatagram {
    socket: Rc<RefCell<UdpSocket>>,
    bytes: Vec<u8>,
    done: bool
}

impl ReceiveDatagram {
    fn new(socket: Rc<RefCell<UdpSocket>>) -> ReceiveDatagram {
        ReceiveDatagram {
            socket: socket,
            bytes: vec![0; 2048],
            done: false
        }
    }
}

impl Future for ReceiveDatagram {
    type Item = (Vec<u8>, SocketAddr);
    type Error = errors::Error;

    fn poll(&mut self) -> Poll<(Vec<u8>, SocketAddr), errors::Error> {
        match self.socket.borrow_mut().recv_from(&mut self.bytes) {
            Ok((0, _)) => Poll::NotReady,
            Ok((n, a)) => {
                println!("read {} bytes from {}.", n, a);
                self.done = true;
                Poll::Ok((self.bytes[..n].to_vec(), a))
            },
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                Poll::NotReady
            }
            Err(e) => Poll::Err(errors::Error::Io(e)),
        }
    }
}

#[derive (Debug)]
struct DatagramListener {
    sock: Rc<RefCell<futures_mio::UdpSocket>>,
    buf: Vec<u8>,
    done: bool
}

impl DatagramListener {
    fn new( sock: Rc<RefCell<futures_mio::UdpSocket>>) -> DatagramListener {
        DatagramListener {
            sock: sock,
            done: false,
            buf: vec![0;2048]
        }
    }
}

impl Stream for DatagramListener {
    type Item = MessageData;
    type Error = errors::Error;

    fn poll(&mut self) -> Poll<Option<MessageData>, errors::Error> {
        match self.sock.borrow_mut().recv_from(&mut self.buf) {
            Ok((0, _)) => (),
            Ok((n, a)) => {
                println!("read {} bytes from {}.", n, a);
                let msg = match dns::Message::new(&self.buf[..n]) {
                    Ok(m) => m,
                    Err(e) => return Poll::Err(errors::Error::DnsParsingError(e))
                };
                let msg = MessageData {
                    message: msg,
                    bytes: self.buf[..n].to_vec(),
                    sender_address: a
                };
                return Poll::Ok(Some(msg))
            },
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                return Poll::NotReady
            }
            Err(e) => return Poll::Err(errors::Error::Io(e)),
        }

        Poll::NotReady
    }
}

use std::collections::HashMap;

struct Client {
    addr: SocketAddr,
    bytes: Vec<u8>
}

struct Server {
    requests: HashMap<u16, Client>
}

use chan;
use futures::stream;
use std::thread;

fn serve(socket: net::UdpSocket, ctl_stream: stream::Receiver<u32,errors::Error>) {
    let mut l = futures_mio::Loop::new().unwrap();

    let server = UdpSocket::from_socket(socket, l.handle());
    let socket = Rc::new(RefCell::new(l.run(server).unwrap()));
    println!("Bound UDP Socket: {:?}", socket);

    let socket2 = socket.clone();

    let server = Rc::new(RefCell::new(Server{
        requests: HashMap::new()
    }));

    let server2 = server.clone();

    let outbound = l.handle().udp_bind(&"0.0.0.0:0".parse().unwrap());
    let outbound = Rc::new(RefCell::new(l.run(outbound).unwrap()));

    let upstream_addr: SocketAddr = "8.8.8.8:53".parse().unwrap();
    let upstream2_addr: SocketAddr = "8.8.4.4:53".parse().unwrap();

    let dg = DatagramListener::new(socket.clone())
        .and_then(move |msg| {
            server.borrow_mut().requests.insert(msg.message.tx_id, Client{
                addr: msg.sender_address,
                bytes: msg.bytes.clone()
            });
            println!("stuffed message with tx_id: {:x} from {}", msg.message.tx_id, msg.sender_address);

            let s1 = SendDatagram::new(outbound.clone(),
                                       msg.bytes.clone(),
                                       upstream_addr.clone());
            let s2 = SendDatagram::new(outbound.clone(),
                                       msg.bytes,
                                       upstream2_addr.clone());
            s1.join(s2)
        })
        .and_then(|(socket,socket2)| {
            let r1 = ReceiveDatagram::new(socket);
            let r2 = ReceiveDatagram::new(socket2);
            r1.join(r2)
        }).and_then(|(r,r2)| {
            let msg = dns::Message::new(&r.0).unwrap();
            let s = server2.borrow();
            let client = s.requests.get(&msg.tx_id).unwrap();

            let s = SendDatagram::new(socket2.clone(),
                                      r.0,
                                      client.addr);
            s
        })
        .for_each(|msg| {
            println!("got a {:?}", msg);
            Ok(())
        });

    l.run(dg).unwrap();
    println!("futures exiting...");
}


pub fn run_server(socket: net::UdpSocket) -> (thread::JoinHandle<()>, stream::Sender<u32,errors::Error>, chan::Receiver<i32> ){
    let (end_sender, rx) = chan::sync(0);
    let (sender, stream) = stream::channel::<u32,errors::Error>();

    let thr = thread::spawn(move || {
        serve(socket, stream);
        chan_select!{
            default => {},
            end_sender.send(0) => {}
        }
    });
    (thr, sender, rx)
}
