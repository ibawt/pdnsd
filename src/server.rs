use mio::udp::*;
use mio::util::*;
use mio::{Token, EventLoop, EventSet, Handler, PollOpt};
use std::net::SocketAddr;
use std::io;
use std::collections::VecDeque;
use bytes::{Buf};
use dns::*;
use buf::*;

const SERVER: Token = Token(1);

#[derive (Debug, Copy, Clone, PartialEq)]
enum QueryState {
    Reset,
    QuestionUpstream,
    WaitingForAnswer,
    AnswerReady
}

#[derive (Debug)]
struct Query {
    token: Token,
    socket: UdpSocket,
    client_addr: Option<SocketAddr>,
    buffer: ByteBuf,
    state: QueryState,
}

impl Query {
    fn new(t: Token) -> io::Result<Query> {
        UdpSocket::v4().map(|socket|
                                 Query {
                                     token: t,
                                     socket: socket,
                                     client_addr: None,
                                     buffer: ByteBuf::new(),
                                     state: QueryState::Reset
                                 })
    }

    fn register(&self, event_loop: &mut EventLoop<Server>) -> Result<(), io::Error> {
        let mut event_set = match self.buffer.get_mode() {
            Mode::Reading => EventSet::writable(),
            Mode::Writing => EventSet::readable(),
            _ => EventSet::none()
        };

        if self.state == QueryState::AnswerReady {
            return Ok(())
        }

        event_loop.reregister(&self.socket, self.token,
                              event_set, PollOpt::edge() | PollOpt::oneshot())
    }

    fn question_upstream(&mut self) {
        match self.state {
            QueryState::Reset => {
                self.buffer.flip();
            },
            _ => panic!("invalid state")
        }
        self.state = QueryState::QuestionUpstream;
    }


    fn is_answer_ready(&self) -> bool {
        match self.state {
            QueryState::AnswerReady => true,
            _ => false
        }
    }
    fn reset(&mut self) {
        self.state = QueryState::Reset;
        self.buffer.set_writable();
        self.client_addr  = None;
    }

    fn send(&mut self, d: &SocketAddr) {
        let size = self.socket.send_to(&mut self.buffer, &d).unwrap();
    }

    fn ready(&mut self, event_loop: &mut EventLoop<Server>, events: EventSet) {
        use self::QueryState::*;
        match self.state {
            Reset => panic!("we should be initialized by now"),
            QuestionUpstream => {
                if events.is_writable() {
                    let dest = "8.8.8.8:53".parse().unwrap();
                    self.send(&dest);
                    self.buffer.flip();
                    self.state = WaitingForAnswer;
                }
            },
            WaitingForAnswer => {
                if events.is_readable() {
                    let result = self.socket.recv_from(&mut self.buffer);

                    match result {
                        Ok(Some(remote_addr)) => {
                            self.buffer.flip();
                            self.state = AnswerReady;
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
                println!("anwser ready");
            }
        }
        self.register(event_loop);
    }
}

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
            queries: Slab::new_starting_at(Token(2), 1024)
        }
    }
}
impl Handler for Server {
    type Timeout = ();
    type Message = ();

    fn ready(&mut self, event_loop: &mut EventLoop<Server>, token: Token, events: EventSet) {
        if token == SERVER {
            if events.is_readable() {
                let token = self.queries.insert_with(|token| Query::new(token).unwrap()).unwrap();

                let mut query = &mut self.queries[token];
                query.reset();

                let result = self.socket.recv_from(&mut query.buffer);

                match result {
                    Ok(Some(remote_addr)) => {
                        {
                            let buf: &Buf = &query.buffer;
                            let msg = Parser::parse(buf.bytes()).unwrap();
                        }
                        query.client_addr = Some(remote_addr);
                        query.question_upstream();
                        query.register(event_loop);
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
                    let dest = self.queries[t].client_addr.unwrap();
                    self.socket.send_to(&mut self.queries[t].buffer, &dest).unwrap();
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
