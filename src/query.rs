use dns::*;
use mio::udp::*;
use std::net::SocketAddr;
use buf::*;
use mio::{Token, EventLoop, EventSet, Handler, PollOpt};

#[derive (Debug, Copy, Clone, PartialEq)]
enum QueryState {
}

#[derive (Debug)]
pub struct Query {
    message: Message,
    addr: SocketAddr,
    token: Token,
    bytes: Vec<u8>,
    upstream_tokens: Vec<Token>,
    upstream_answers: Vec<Message>
}

impl Query {
    pub fn new(client_addr: SocketAddr, t: Token, msg: Message) -> Query {
        Query {
            bytes: vec![],
            message: msg,
            token: t,
            addr: client_addr,
            upstream_tokens: vec![],
            upstream_answers: vec![]
        }
    }

    pub fn copy_message_bytes(&mut self, bytes: &[u8]) {
        self.bytes.clear();
        for i in bytes.iter() {
            self.bytes.push(*i);
        }
    }

    pub fn question_bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn add_upstream_token(&mut self, t: Token) {
        self.upstream_tokens.push(t);
    }

    pub fn is_valid_response(&self, bytes: &[u8]) -> bool {
        parse_txn_id(bytes)
            .map_or(false, |txn_id| txn_id == self.message.tx_id)
    }
}
