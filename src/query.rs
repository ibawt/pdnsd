use dns::*;
use std::net::SocketAddr;
use mio::{Token};

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
    message: Message,
    addr: SocketAddr,
    token: Token,
    bytes: Vec<u8>,
    upstreams: Vec<Upstream>,
    done: bool
}

impl Query {
    pub fn new(client_addr: SocketAddr, t: Token, msg: Message) -> Query {
        Query {
            bytes: vec![],
            message: msg,
            token: t,
            addr: client_addr,
            upstreams: vec![],
            done: false
        }
    }

    fn find_upstream(&mut self, t: Token) -> Option<&mut Upstream> {
        self.upstreams.iter_mut().find(|x| x.token == t)
    }

    pub fn get_addr(&self) -> &SocketAddr {
        &self.addr
    }

    pub fn transmit_done(&mut self, t: Token, size: usize) -> bool {
        if self.done {
            return false
        }
        let len = self.bytes.len();
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
        println!("input array is {} bytes", bytes.len());
        let tx_id = self.message.tx_id;
        let mut upstream = self.find_upstream(t).unwrap();
        if let Ok(msg) = Message::new(bytes) {
            println!("in here?");
            if msg.tx_id == tx_id {
                upstream.phase = QueryPhase::ResponseReady;
                return true
            }
            println!("after clause")
        }
        println!("guess it didn't parse right");
        false
    }

    pub fn copy_message_bytes(&mut self, bytes: &[u8]) {
        self.bytes.clear();
        for i in bytes.iter() {
            self.bytes.push(*i);
        }
    }

    pub fn is_done(&self) -> bool {
        self.done
    }

    pub fn question_bytes(&self) -> &[u8] {
        &self.bytes[0..self.bytes.len()]
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
            .map_or(false, |txn_id| txn_id == self.message.tx_id)
    }
}
