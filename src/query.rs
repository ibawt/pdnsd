use dns::*;
use std::net::SocketAddr;
use mio::{Token};
use buf::*;
use std::io::Write;
use mio::udp::UdpSocket;

#[derive (Debug, Copy, Clone, PartialEq)]
enum QueryPhase {
    Waiting,
    SendRequest,
    WaitResponse,
    ResponseReady
}

#[derive (Debug, Clone)]
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
    done: bool
}
use std::io;

impl Query {
    pub fn new() -> Query {
        Query {
            bytes: ByteBuf::new(),
            message: None,
            addr: None,
            upstreams: vec![],
            done: false
        }
    }

    pub fn rx(&mut self, s: &UdpSocket) -> io::Result<Option<()>> {
        self.bytes.set_writable();
        match try!(s.recv_from(self.bytes.mut_bytes())) {
            Some((size, addr)) => {
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

    pub fn rx_buf(&mut self) -> &mut [u8] {
        self.bytes.mut_bytes()
    }

    pub fn parse_message(&mut self) {
        self.message = Message::new(self.question_bytes()).ok();
    }

    pub fn set_len(&mut self, i: usize) {
        self.bytes.advance(i)
    }

    fn find_upstream(&mut self, t: Token) -> Option<&mut Upstream> {
        self.upstreams.iter_mut().find(|x| x.token == t)
    }

    pub fn get_addr(&self) -> &Option<SocketAddr> {
        &self.addr
    }

    pub fn transmit_done(&mut self, t: Token, size: usize) -> bool {
        if self.done {
            return false
        }
        let len = self.bytes.bytes().len();
        let upstream = self.find_upstream(t).unwrap();
        if upstream.phase == QueryPhase::SendRequest {
            if size == len {
                upstream.phase = QueryPhase::WaitResponse;
                return true
            }
            return false
        }
        else {
            // if size == size of message
            true
        }
    }

    pub fn set_done(&mut self) {
        self.done = true;
    }

    pub fn recv_done(&mut self, t: Token, addr: SocketAddr, bytes: &[u8]) -> bool {
        let tx_id = self.message.as_ref().map(|m| m.tx_id).unwrap();
        let mut upstream = self.find_upstream(t).unwrap();
        if let Ok(msg) = Message::new(bytes) {
            if msg.tx_id == tx_id {
                upstream.phase = QueryPhase::ResponseReady;
                return true
            }
        }
        false
    }

    pub fn copy_message_bytes(&mut self, bytes: &[u8]) {
        self.bytes.set_writable();
        self.bytes.write_all(bytes).unwrap();
    }

    pub fn is_done(&self) -> bool {
        self.done
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

    pub fn is_valid_response(&self, bytes: &[u8]) -> bool {
        parse_txn_id(bytes)
            .map_or(false, |txn_id| txn_id == self.message.as_ref().unwrap().tx_id)
    }
}
