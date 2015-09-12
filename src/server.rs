use mio::udp::*;
use mio::util::*;
use mio::{Token, EventLoop, EventSet, Handler, PollOpt};
use std::net::SocketAddr;
use bytes::*;

use dns::*;

const SERVER: Token = Token(0);

#[derive (Debug, Copy, Clone, PartialEq)]
enum QueryState {
    Reset,
    QuestionUpstream,
    WaitingForAnswer,
    AnswerReady
}

const BUF_LEN: usize = 16384;

struct Query {
    token: Token,
    upstream: UdpSocket,
    from: Option<SocketAddr>,
    buf: Option<ByteBuf>,
    mut_buf: Option<MutByteBuf>,
    state: QueryState,
}

impl Query {
    fn new(t: Token) -> Query {
        Query {
            token: t,
            upstream: UdpSocket::v4().unwrap(),
            from: None,
            mut_buf: Some(ByteBuf::mut_with_capacity(BUF_LEN)),
            buf: None,
            state: QueryState::Reset
        }
    }

    fn register(&self, event_loop: &mut EventLoop<Server>) {
        use self::QueryState::*;

        let event_set = match self.state {
            QuestionUpstream => EventSet::writable(),
            WaitingForAnswer => EventSet::readable(),
            _ => return
        };

        event_loop.reregister(&self.upstream, self.token,
                              event_set, PollOpt::edge() | PollOpt::oneshot())
            .unwrap();
    }

    fn to_writable(&mut self) {
        if self.mut_buf.is_none() {
            self.mut_buf = self.buf.take().map(|b| b.flip());
        }
    }

    fn question_upstream(&mut self) {
        match self.state {
            QueryState::Reset => {
                self.to_readable();
            },
            _ => panic!("invalid state")
        }
        self.state = QueryState::QuestionUpstream;
    }

    fn to_readable(&mut self) {
        if self.buf.is_none() {
            self.buf = self.mut_buf.take().map(|b| b.flip())
        }
    }

    fn is_answer_ready(&self) -> bool {
        match self.state {
            QueryState::AnswerReady => true,
            _ => false
        }
    }

    fn reset(&mut self) {
        use self::QueryState::*;

        match self.state {
            QuestionUpstream => self.to_writable(),
            AnswerReady => self.to_writable(),
            _ => ()
        }
        self.from = None;
        self.upstream = UdpSocket::v4().unwrap();
    }

    fn send(&mut self, d: &SocketAddr) {
        assert!(self.buf.is_some());
        if let Some(ref mut b) = self.buf {
            self.upstream.send_to(b, &d).unwrap();
        }
    }

    fn ready(&mut self, event_loop: &mut EventLoop<Server>, events: EventSet) {
        use self::QueryState::*;

        match self.state {
            Reset => panic!("we should be initialized by now"),
            QuestionUpstream => {
                if events.is_writable() {
                    println!("query -- question upstream...");
                    let dest = "8.8.8.8:53".parse().unwrap();
                    self.send(&dest);
                    self.to_writable();
                    self.state = WaitingForAnswer;
                }
            },
            WaitingForAnswer => {
                if events.is_readable() {
                    println!("reading upstream answer");
                    let result = match self.mut_buf {
                        Some(ref mut b) => self.upstream.recv_from(b),
                        _ => panic!("argh")
                    };

                    match result {
                        Ok(Some(remote_addr)) => {
                            println!("read from {:?}", remote_addr);
                            self.state = AnswerReady;
                            self.to_readable();
                            println!("answer ready!");
                        },
                        Ok(None) => {
                            // try again
                        },
                        Err(_) => {
                            panic!("error")
                        }
                    }
                }
            },
            AnswerReady => {
            }
        }
        self.register(event_loop);
    }
}

use std::collections::VecDeque;

#[derive (Debug)]
struct Server {
    write_queue: VecDeque<Token>,
    socket: UdpSocket,
    queries: Slab<Query>
}

impl Server {
    fn new(s: UdpSocket) -> Server {
        Server{
            write_queue: VecDeque::new(),
            socket: s,
            queries: Slab::new_starting_at(Token(1), 1024)
        }
    }
}

impl Handler for Server {
    type Timeout = ();
    type Message = ();

    fn ready(&mut self, event_loop: &mut EventLoop<Server>, token: Token, events: EventSet) {
        if token == SERVER {
            if events.is_readable() {
                let token = self.queries.insert_with(|token| Query::new(token)).unwrap();

                self.queries[token].reset();

                let result = match self.queries[token].mut_buf {
                    Some(ref mut b) => self.socket.recv_from(b),
                    _ => panic!("blah")
                };

                match result {
                    Ok(Some(remote_addr)) => {
                        self.queries[token].from = Some(remote_addr);
                        self.queries[token].question_upstream();
                        self.queries[token].register(event_loop);
                    },
                    Ok(None) => {
                        println!("none!");
                    }
                    Err(_) => {
                        panic!("error!");
                    }
                }
            }

            if events.is_writable() {
                if let Some(t) = self.write_queue.pop_front() {
                    println!("popping {:?} off write_queue", t);
                    let dest = self.queries[t].from.unwrap();
                    if let Some(ref mut b) = self.queries[t].buf {
                        self.socket.send_to(b, &dest).unwrap();
                    }
                    event_loop.deregister(&self.queries[t].upstream).unwrap();
                    self.queries[t].reset();
                    self.queries.remove(t);

                    if self.write_queue.is_empty() {
                        event_loop.reregister(&self.socket, SERVER, EventSet::readable(),
                                              PollOpt::level()).unwrap();
                    }
                }
            }
        } else {
            self.queries[token].ready(event_loop, events);

            if self.queries[token].is_answer_ready() {
                self.write_queue.push_back(token);
                event_loop.reregister(&self.socket, SERVER, EventSet::readable()
                                      | EventSet::writable(),
                                      PollOpt::level()).unwrap();
            }
        }
    }
}

pub fn run_server(s: UdpSocket) {
    let mut evt_loop = EventLoop::new().ok().expect("event loop failed");

    evt_loop.register_opt(&s, SERVER, EventSet::readable(), PollOpt::level())
        .ok().expect("registration failed");

    evt_loop.run(&mut Server::new(s)).ok().expect("event loop run");
}


#[cfg(test)]
mod tests {
    use self::super::*;
    use dns::*;
    use std::net::*;
    use mio;
    use std::thread;

    fn test_dns_request(b: &[u8], dest: &SocketAddr) -> [u8;512] {
        let local: SocketAddr = "0.0.0.0:0".parse().unwrap();
        let socket = UdpSocket::bind(&local).unwrap();

        let size = socket.send_to(b, &dest).unwrap();

        let mut bytes = [0u8; 512];

        let (bytes_written, recv_socket) = socket.recv_from(&mut bytes).unwrap();

        bytes
    }

    #[test]
    fn simple_proxy() {
        let request = include_bytes!("../test/dns_request.bin");

        let google_dns = "8.8.8.8:53".parse().unwrap();

        let response = test_dns_request(request, &google_dns);

        let msg = Parser::parse(&response).unwrap();

        let server_thread = thread::spawn(move || {
            let server_addr = "0.0.0.0:9080".parse().unwrap();
            let s = mio::udp::UdpSocket::bound(&server_addr).unwrap();
            run_server(s);
        });


        let t = thread::spawn(move || {
            let addr: SocketAddr = "0.0.0.0:9080".parse().unwrap();
            let request = include_bytes!("../test/dns_request.bin");
            let output = test_dns_request(request, &addr);
            let msg = Parser::parse(&output).unwrap();

            msg
        });

        let res = t.join().unwrap();

        assert_eq!(msg.tx_id, res.tx_id);
        assert_eq!(msg.answers.len(), res.answers.len());
        assert_eq!(msg.answers[0].r_name, res.answers[0].r_name);
        assert_eq!(msg.answers[0].r_type, res.answers[0].r_type);
        assert_eq!(msg.answers[0].r_class, res.answers[0].r_class);
    }
}
