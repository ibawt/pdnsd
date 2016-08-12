use mio::{Token, EventSet, PollOpt, Handler, EventLoop};
use mio::udp::UdpSocket;
use std::net::SocketAddr;
use std::io::prelude::*;
use errors::*;
use buf::{ByteBuf};
use std::io;

#[derive (Debug)]
pub struct Datagram {
    token: Token,
    query_token: Token,
    socket_addr: SocketAddr,
    socket: UdpSocket,
    buf: ByteBuf,
    state: State
}

pub enum EventResponse {
    Tx(Option<usize>),
    Rx(Option<(usize, SocketAddr)>),
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

    pub fn socket(&self) -> &UdpSocket {
        &self.socket
    }

    pub fn state(&self) -> State {
        self.state
    }

    pub fn set_addr(&mut self, addr: SocketAddr) {
        self.socket_addr = addr;
    }

    pub fn get_addr(&self) -> &SocketAddr {
       &self.socket_addr
    }

    pub fn query_token(&self) -> Token {
        self.query_token
    }

    pub fn token(&self) -> Token {
        self.token
    }

    pub fn fill(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.buf.clear();
        self.buf.set_writable();
        try!(self.buf.write_all(bytes));
        self.buf.flip();
        self.state = State::Tx;
        Ok(())
    }


    // pub fn set_tx(&mut self) {
    //     self.state = State::Tx;
    //     self.buf.set_readable();
    // }

    pub fn set_rx(&mut self) {
        self.state = State::Rx;
        self.buf.set_writable();
    }

    pub fn set_idle(&mut self) {
        self.state = State::Idle;
    }

    pub fn get_ref(&self) -> &[u8] {
        self.buf.bytes()
    }

    pub fn event(&mut self, events: EventSet) -> Result<EventResponse, Error> {
        match self.state {
            State::Tx => {
                // if the buf is readable we are TX'ing
                if events.is_writable() {
                    match try!(self.socket.send_to(self.buf.bytes(), &self.socket_addr)) {
                        Some(size) => {
                            self.buf.advance(size); // This should be transparent here
                            Ok(EventResponse::Tx(Some(size)))
                        },
                        None => Ok(EventResponse::Tx(None)),
                    }
                } else {
                    Err(Error::String("invalid state"))
                }
            },
            State::Rx => {
                if events.is_readable() {
                    match self.socket.recv_from(self.buf.mut_bytes()) {
                        Ok(Some((size, addr))) => {
                            self.buf.advance(size);
                            Ok(EventResponse::Rx(Some((size, addr))))
                        }
                        Ok(None) => {
                            Ok(EventResponse::Rx(None))
                        },
                        Err(e) => Err(Error::Io(e))
                    }
                } else {
                    Err(Error::String("invalid state"))
                }
            }
            _ => {
                Ok(EventResponse::Nothing)
            }
        }
    }

    pub fn reregister<H: Handler>(&self, event_loop: &mut EventLoop<H>) -> io::Result<()> {
        let (event_set, poll_opt) = self.event_set_poll_opts();
        event_loop.reregister(&self.socket, self.token, event_set, poll_opt)
    }

    pub fn register<H: Handler>(&self, event_loop: &mut EventLoop<H>) -> io::Result<()> {
        let (event_set, poll_opt) = self.event_set_poll_opts();

        event_loop.register(&self.socket, self.token, event_set, poll_opt)
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
