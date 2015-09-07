use mio::udp::*;
use mio::util::*;
use mio::{Token, EventLoop, EventSet, Handler};
use std::net::SocketAddr;
use bytes::*;

const SERVER: Token = Token(0);

#[derive (Debug, PartialEq, Clone, Copy)]
enum QueryState {
    Reset,
    QuestionUpstream,
    WaitingForAnswer,
    AnswerReady
}

const BUF_LEN: usize = 16384;

struct Query {
    upstream: UdpSocket,
    from: Option<SocketAddr>,
    query_buff_mut: Option<MutByteBuf>,
    query_buff: Option<ByteBuf>,
    response_buff_mut: Option<MutByteBuf>,
    response_buff: Option<ByteBuf>,
    state: QueryState,
}

impl Query {
    fn new() -> Query {
        Query {
            upstream: UdpSocket::v4().unwrap(),
            from: None,
            query_buff_mut: Some(ByteBuf::mut_with_capacity(BUF_LEN)),
            query_buff: None,
            response_buff_mut: Some(ByteBuf::mut_with_capacity(BUF_LEN)),
            response_buff: None,
            state: QueryState::Reset
        }
    }

    fn reset(&mut self) {
        match self.query_buff_mut {
            Some(ref mut b) => {
                b.clear();
            },
            _ => {
                self.query_buff_mut = self.query_buff.take().map(|f| f.flip());
            }
        }

        match self.response_buff_mut {
            Some(ref mut b) => {
                b.clear();
            },
            _ => {
                self.response_buff_mut = self.response_buff.take().map(|f| f.flip());
            }
        }
        self.state = QueryState::Reset;
        self.from = None;
        self.upstream = UdpSocket::v4().unwrap();
    }

    fn ready(&mut self, _: &mut EventLoop<Server>, events: EventSet) {
        use self::QueryState::*;

        match self.state {
            Reset => panic!("we should be initialized by now"),
            QuestionUpstream => {
                if events.is_writable() {
                    println!("query -- question upstream...");
                    let dest = "8.8.8.8:53".parse().unwrap();
                    if let Some(ref mut b) = self.query_buff {
                        self.upstream.send_to(b, &dest).unwrap();
                    } else {
                        panic!("dsfjdsl");
                    }
                    self.state = WaitingForAnswer;
                }
            },
            WaitingForAnswer => {
                if events.is_readable() {
                    println!("reading upstream answer");
                    let result = match self.response_buff_mut {
                        Some(ref mut b) => self.upstream.recv_from(b),
                        _ => panic!("blah")
                    };

                    match result {
                        Ok(Some(remote_addr)) => {
                            println!("read from {:?}", remote_addr);
                            self.response_buff = self.response_buff_mut.take().map(|x| x.flip());
                            self.state = AnswerReady;
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
                let token = self.queries.insert_with(|_| Query::new()).unwrap();

                self.queries[token].reset();

                let result = match self.queries[token].query_buff_mut {
                    Some(ref mut b) => self.socket.recv_from(b),
                    _ => panic!("blah")
                };

                match result {
                    Ok(Some(remote_addr)) => {
                        self.queries[token].from = Some(remote_addr);
                        self.queries[token].query_buff = self.queries[token].query_buff_mut.take().map(|f| f.flip());
                        self.queries[token].state = QueryState::QuestionUpstream;
                        event_loop.register(&self.queries[token].upstream, token).ok().expect("upstream socket registered")
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
                    if let Some(ref mut b) = self.queries[t].response_buff {
                        self.socket.send_to(b, &dest).unwrap();
                        println!("sending proxied response back");
                    }
                    println!("deregister!");
                    event_loop.deregister(&self.queries[t].upstream).unwrap();
                    println!("reset");
                    self.queries[t].reset();
                    println!("before remove");
                    self.queries.remove(t);
                }
            }
        } else {
            self.queries[token].ready(event_loop, events);

            if self.queries[token].state == QueryState::AnswerReady {
                self.write_queue.push_back(token);
            }
        }
    }
}

pub fn run_server(s: UdpSocket) {
    let mut evt_loop = EventLoop::new().ok().expect("event loop failed");

    evt_loop.register(&s, SERVER).ok().expect("registration failed");

    evt_loop.run(&mut Server::new(s)).ok().expect("event loop run");
}
