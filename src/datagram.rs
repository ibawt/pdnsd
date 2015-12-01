use mio::{Token, EventSet, PollOpt, Handler, EventLoop};
use mio::udp::UdpSocket;
use std::net::SocketAddr;
use std::io::prelude::*;
use std::io;
use errors::*;
use buf::{ByteBuf};

#[derive (Debug)]
pub struct Datagram {
    token: Token,
    query_token: Token,
    socket_addr: SocketAddr,
    socket: UdpSocket,
    buf: ByteBuf,
    state: State
}

pub type TransmitResponse = Option<usize>;
pub type ReceiveResponse = Option<(usize, SocketAddr)>;

pub enum DatagramEventResponse {
    Transmit(TransmitResponse),
    Recv(ReceiveResponse),
    Nothing
}

#[derive (Debug, PartialEq, Clone, Copy)]
pub enum State {
    Tx,
    Rx,
    Idle
}

impl Datagram {
    pub fn new(t: Token, qt: Token, remote: SocketAddr) -> Datagram {
        Datagram{
            token: t,
            query_token: qt,
            socket_addr: remote,
            socket: UdpSocket::v4().unwrap(),
            buf: ByteBuf::new(),
            state: State::Idle
        }
    }

    pub fn set_addr(&mut self, addr: SocketAddr) {
        self.socket_addr = addr;
    }

    pub fn query_token(&self) -> Token {
        self.query_token
    }

    pub fn fill(&mut self, bytes: &[u8]) {
        self.buf.clear();
        self.buf.set_writable();
        self.buf.write_all(bytes).unwrap();
        self.buf.flip();
        self.state = State::Tx;
    }

    pub fn set_tx(&mut self) {
        // we want to READ data IN so we set teh buffer to be WRITABLE
        self.state = State::Tx;
        self.buf.set_readable();
    }

    pub fn set_rx(&mut self) {
        self.state = State::Rx;
        // we want to SEND data so the buffer is readable
        self.buf.set_writable();
    }

    pub fn set_idle(&mut self) {
        self.state = State::Idle;
    }

    pub fn get_ref(&self) -> &[u8] {
        self.buf.bytes()
    }

    fn transmit(&mut self) -> io::Result<Option<usize>> {
        self.socket.send_to(self.buf.bytes(), &self.socket_addr)
    }

    fn recv(&mut self) -> io::Result<Option<(usize, SocketAddr)>> {
        {
            println!("mut len: {}", self.buf.mut_bytes().len());
        }
        self.socket.recv_from(self.buf.mut_bytes())
    }

    pub fn event(&mut self, events: EventSet) -> Result<DatagramEventResponse, Error> {
        match self.state {
            State::Tx => {
                // if the buf is readable we are TX'ing
                if events.is_writable() {
                    println!("dg transmitting");
                    self.transmit().map(|size| DatagramEventResponse::Transmit(size))
                        .map_err(|e| Error::Io(e))
                } else {
                    Err(Error::String("invalid state"))
                }
            },
            State::Rx => {
                if events.is_readable() {
                    println!("dg rx'ing");
                    match self.recv() {
                        Ok(Some((size, addr))) => {
                            println!("rx'd {}", size);
                            self.buf.advance(size);
                            Ok(DatagramEventResponse::Recv(Some((size, addr))))
                        },
                        Ok(None) => {
                            Ok(DatagramEventResponse::Recv(None))
                        },
                        Err(e) => Err(Error::Io(e))
                    }
                    // self.recv().map(|t| DatagramEventResponse::Recv(t))
                    //     .map_err(|e| Error::Io(e))
                } else {
                    Err(Error::String("invalid state"))
                }
            },
            _ => {
                Ok(DatagramEventResponse::Nothing)
            }
        }
    }

    pub fn reregister<H: Handler>(&self, event_loop: &mut EventLoop<H>) {
        let (event_set, poll_opt) = self.event_set_poll_opts();

        println!("{:?}: reregister event_set: {:?}", self.token, event_set);
        event_loop.reregister(&self.socket, self.token, event_set, poll_opt).unwrap();
    }

    pub fn register<H: Handler>(&self, event_loop: &mut EventLoop<H>) {
        let (event_set, poll_opt) = self.event_set_poll_opts();

        println!("{:?}: initial register event_set: {:?}", self.token, event_set);
        event_loop.register(&self.socket, self.token, event_set, poll_opt).unwrap();
    }

    fn event_set_poll_opts(&self) -> (EventSet, PollOpt) {
        match self.state {
            State::Tx => {
                (EventSet::writable(), PollOpt::edge() | PollOpt::oneshot())
            },
            State::Rx => {
                (EventSet::readable(), PollOpt::edge() | PollOpt::oneshot())
            },
            State::Idle => {
                (EventSet::none(), PollOpt::empty())
            }
        }
    }
}
