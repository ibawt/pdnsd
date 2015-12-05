use dns::*;
use errors;
use std::net::SocketAddr;
use mio::{Token, EventSet};
use buf::*;
use std::io::Write;
use mio::udp::UdpSocket;
use datagram::*;

#[derive (Debug, Copy, Clone, PartialEq)]
enum QueryPhase {
    Waiting,
    SendRequest,
    WaitResponse,
    ResponseReady
}

#[derive (Debug)]
struct Upstream {
    token: Token,
    answer: Message,
    phase: QueryPhase
}

#[derive (Debug)]
pub struct Query {
    message: Option<Message>,
    addr: Option<SocketAddr>,
    bytes: ByteBuf,
    upstreams: Vec<Upstream>,
}
use std::io;

impl Query {
    pub fn new() -> Query {
        Query {
            bytes: ByteBuf::new(),
            message: None,
            addr: None,
            upstreams: vec![],
        }
    }

    pub fn rx(&mut self, s: &UdpSocket) -> io::Result<Option<()>> {
        self.bytes.set_writable();
        match try!(s.recv_from(self.bytes.mut_bytes())) {
            Some((size, addr)) => {
                info!("accepting connection from: {}", addr);
                self.bytes.set_pos(size as i32);
                self.message = Message::new(self.bytes.bytes()).ok();
                self.addr = Some(addr);
                Ok(Some(()))
            },
            None => {
                Ok(None)
            }
        }
    }

    pub fn upstream_tokens(&self) -> Vec<Token> {
        self.upstreams.iter().map(|up| up.token ).collect()
    }

    pub fn rx_buf(&mut self) -> &mut [u8] {
        self.bytes.mut_bytes()
    }

    fn find_upstream(&self, t: Token) -> Option<usize> {
        self.upstreams.iter().position(|x| x.token == t)
    }

    pub fn get_addr(&self) -> Option<&SocketAddr> {
        self.addr.as_ref()
    }

    fn send_request_phase(&mut self, datagram: &mut Datagram, event_response: EventResponse) -> Result<bool, errors::Error> {
        match event_response {
            EventResponse::Tx(Some(size)) => {
                if size == self.question_bytes().len() {
                    Ok(true)
                } else {
                    Ok(false)
                }
            }
            _ => {
                try!(Err("invalid response!"))
            }
        }
    }

    fn wait_response_phase(&mut self, datagram: &mut Datagram, event_response: EventResponse) -> Result<bool, errors::Error> {
        match event_response {
            EventResponse::Rx(Some(addr)) => {
                let upstream = try!(self.find_upstream(datagram.token()).ok_or("no upstream for datagram!"));
                self.upstreams[upstream].answer = try!(Message::new(datagram.get_ref()));

                let tx_id = try!(self.message.as_ref().ok_or("no message!")).tx_id;
                if tx_id != self.upstreams[upstream].answer.tx_id {
                    try!(Err("invalid tx_id!"));
                }
                Ok(true)
            },
            _ => {
                try!(Err("invalid response!"))
            }
        }
    }

    /// State Machine Enter!
    pub fn datagram_event(&mut self, datagram: &mut Datagram, events: EventSet) -> Result<bool, errors::Error> {
        // first find the thing this is for and see where it's' at
        let upstream = try!(self.find_upstream(datagram.token()).ok_or("no upstream for datagram!"));
        // actually do the datagram event
        let event_response = try!(datagram.event(events));

        match self.upstreams[upstream].phase {
            QueryPhase::SendRequest => {
                // Send To Upstream Server
                assert!(events.is_writable());
                return self.send_request_phase(datagram, event_response).and_then(|success| {
                    if success {
                        // transition to next state
                        self.upstreams[upstream].phase = QueryPhase::WaitResponse;
                        datagram.set_rx();
                    }
                    Ok(false)
                })
            },
            QueryPhase::WaitResponse => {
                assert!(events.is_readable());
                return self.wait_response_phase(datagram, event_response).and_then(|success| {
                    if success {
                        self.upstreams[upstream].phase = QueryPhase::ResponseReady;
                        datagram.set_idle();
                        try!(self.copy_message_bytes(datagram.get_ref()));
                        Ok(true)
                    } else {
                        Ok(false)
                    }
                })
            },
            QueryPhase::ResponseReady => {
                println!("response phase shouldn't get events");
            },
            QueryPhase::Waiting => {
                println!("waiting phase shouldn't get events!");
            }
        }
        Err(errors::Error::String("ARG"))
    }

    pub fn copy_message_bytes(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.bytes.set_writable();
        self.bytes.write_all(bytes)
    }

    pub fn question_bytes(&self) -> &[u8] {
        self.bytes.bytes()
    }

    pub fn add_upstream_token(&mut self, t: Token) {
        let upstream = Upstream{
            token: t,
            answer: Message::default(),
            phase: QueryPhase::SendRequest
        };
        self.upstreams.push(upstream);
    }
}
