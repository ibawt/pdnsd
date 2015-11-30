use mio::{Token, EventSet, PollOpt, Handler, EventLoop};
use mio::udp::UdpSocket;
use std::net::SocketAddr;
use std::io;
use std::io::{Cursor, Write};
use errors::*;

#[derive (Debug, PartialEq)]
enum Mode {
    Reading,
    Writing,
    Idle
}

#[derive (Debug)]
pub struct Datagram {
    token: Token,
    query_token: Token,
    socket_addr: SocketAddr,
    socket: UdpSocket,
    buf: Cursor<Vec<u8>>,
    mode: Mode
}

pub type TransmitResponse = Option<usize>;
pub type ReceiveResponse = Option<(usize, SocketAddr)>;

pub enum DatagramEventResponse {
    Transmit(TransmitResponse),
    Recv(ReceiveResponse),
    Nothing
}

impl Datagram {
    pub fn new(t: Token, qt: Token, remote: SocketAddr) -> Datagram {
        Datagram{
            token: t,
            query_token: qt,
            socket_addr: remote,
            socket: UdpSocket::v4().unwrap(),
            buf: Cursor::new(Vec::with_capacity(512)),
            mode: Mode::Reading
        }
    }

    pub fn set_addr(&mut self, addr: SocketAddr) {
        self.socket_addr = addr;
    }

    pub fn query_token(&self) -> Token {
        self.query_token
    }

    pub fn fill(&mut self, bytes: &[u8]) {
        self.buf.set_position(0);
        self.buf.write_all(bytes).unwrap();
        self.mode = Mode::Writing;
    }

    pub fn set_reading(&mut self) {
        self.reset();
        self.mode = Mode::Reading;
    }

    pub fn set_writing(&mut self) {
        self.reset();
        self.mode = Mode::Writing;
    }

    pub fn set_idle(&mut self) {
        self.mode = Mode::Idle;
    }

    fn reset(&mut self) {
        self.buf.set_position(0);
    }

    pub fn get_ref(&self) -> &[u8] {
        self.buf.get_ref()
    }

    fn transmit(&mut self) -> io::Result<Option<usize>> {
        self.socket.send_to(self.buf.get_ref(), &self.socket_addr)
    }

    fn recv(&mut self) -> io::Result<Option<(usize, SocketAddr)>> {
        let mut buf =  [0u8 ; 512 ];

        match self.socket.recv_from(&mut buf)  {
            Ok(Some((size, addr))) => {
                self.buf.write_all(&buf[0..size]).unwrap();
                Ok(Some((size,addr)))
            }
            Ok(None) => Ok(None),
            Err(e) => Err(e)
        }
    }

    pub fn event(&mut self, events: EventSet) -> Result<DatagramEventResponse, Error> {
        match self.mode {
            Mode::Writing => {
                if events.is_writable() {
                    println!("dg transmitting");
                    self.transmit().map(|size| DatagramEventResponse::Transmit(size))
                        .map_err(|e| Error::Io(e))
                } else {
                    Err(Error::String("invalid state"))
                }
            },
            Mode::Reading => {
                if events.is_readable() {
                    println!("dg rx'ing");
                    self.recv().map(|t| DatagramEventResponse::Recv(t))
                        .map_err(|e| Error::Io(e))
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

        event_loop.reregister(&self.socket, self.token, event_set, poll_opt).unwrap();
    }

    pub fn register<H: Handler>(&self, event_loop: &mut EventLoop<H>) {
        let (event_set, poll_opt) = self.event_set_poll_opts();

        event_loop.register(&self.socket, self.token, event_set, poll_opt).unwrap();
    }

    fn event_set_poll_opts(&self) -> (EventSet, PollOpt) {
        match self.mode {
            Mode::Writing => {
                (EventSet::writable(), PollOpt::edge() | PollOpt::oneshot())
            },
            Mode::Reading => {
                (EventSet::readable(), PollOpt::edge() | PollOpt::oneshot())
            },
            Mode::Idle => {
                (EventSet::none(), PollOpt::empty())
            }
        }
    }
}
