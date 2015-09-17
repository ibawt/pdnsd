use mio::udp::*;
use mio::util::*;
use mio::{Token, EventLoop, EventSet, Handler, PollOpt};
use std::net::SocketAddr;
use std::io;
use std::collections::VecDeque;
use bytes::{Buf};
use dns::{Parser};
use buf::*;
use std::net;

const SERVER: Token = Token(1);

#[derive (Debug, Copy, Clone, PartialEq)]
enum QueryState {
    WaitingForQuestion,
    QuestionUpstream,
    WaitingForAnswer,
    AnswerReady,
    Closed
}

#[derive (Debug)]
struct Query {
    token: Token,
    socket: UdpSocket,
    client_addr: Option<SocketAddr>,
    buffer: ByteBuf,
    state: QueryState,
    dest_addr: SocketAddr
}

#[derive (Debug)]
enum Error {
    QueryStateError,
    String(&'static str),
    Io(io::Error),
    AddrParseError(net::AddrParseError)
}
use std::fmt;

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            Error::QueryStateError => write!(f, "QueryStateError"),
            Error::Io(ref err) => write!(f, "{:?}", err),
            Error::AddrParseError(ref e) => write!(f, "{:?}", e),
            Error::String(ref s) => write!(f, "{}", s)
        }
    }
}

impl From<io::Error> for Error {
    fn from(io: io::Error) -> Error {
        Error::Io(io)
    }
}

impl From<net::AddrParseError> for Error {
    fn from(a: net::AddrParseError) -> Error {
        Error::AddrParseError(a)
    }
}

impl Query {
    fn new(t: Token) -> Result<Query, Error> {
        let s = try!(UdpSocket::v4());
        let dest = try!("8.8.8.8:53".parse());
        Ok(Query {
            token: t,
            socket: s,
            client_addr: None,
            buffer: ByteBuf::new(),
            state: QueryState::WaitingForQuestion,
            dest_addr: dest
        })
    }

    fn register(&self, event_loop: &mut EventLoop<Server>) -> Result<(), Error> {
        let event_set = match self.state {
            QueryState::AnswerReady | QueryState::Closed => {
                EventSet::none()
            }
            _ => {
                match self.buffer.get_mode() {
                    Mode::Reading => EventSet::writable(),
                    Mode::Writing => EventSet::readable()
                }
            }
        };

        try!(event_loop.reregister(&self.socket, self.token,
                                   event_set, PollOpt::edge() | PollOpt::oneshot()));
        Ok(())
    }

    fn close(&mut self, event_loop: &mut EventLoop<Server>) -> Result<(), Error> {
        self.state = QueryState::Closed;
        event_loop.deregister(&self.socket).map_err(|e| Error::Io(e))
    }

    fn question_upstream(&mut self) -> Result<(), Error> {
        match self.state {
            QueryState::WaitingForQuestion => {
                self.buffer.flip();
            },
            _ => return Err(Error::QueryStateError)
        }
        self.state = QueryState::QuestionUpstream;
        Ok(())
    }

    fn is_answer_ready(&self) -> bool {
        match self.state {
            QueryState::AnswerReady => true,
            _ => false
        }
    }

    fn wait_for_question(&mut self) {
        self.state = QueryState::WaitingForQuestion;
        self.buffer.set_writable();
        self.client_addr  = None;
    }

    fn ready(&mut self, event_loop: &mut EventLoop<Server>, events: EventSet) -> Result<(), Error> {
        use self::QueryState::*;
        match self.state {
            WaitingForQuestion => return Err(Error::QueryStateError),
            QuestionUpstream => {
                if events.is_writable() {
                    try!(self.socket.send_to(&mut self.buffer, &self.dest_addr));
                    self.buffer.flip();
                    self.state = WaitingForAnswer;
                }
            },
            WaitingForAnswer => {
                if events.is_readable() {
                    if let Some(_) = try!(self.socket.recv_from(&mut self.buffer)) {
                        self.buffer.flip();
                        self.state = AnswerReady;
                    }
                }
            },
            _ => ()
        }
        self.register(event_loop)
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

#[derive (Debug)]
enum ServerEvent {
    Quit
}

impl Handler for Server {
    type Timeout = ();
    type Message = ServerEvent;

    fn notify(&mut self, event_loop: &mut EventLoop<Server>, msg: ServerEvent) {
        match msg {
            ServerEvent::Quit => {
                event_loop.shutdown();
            }
        }
    }

    fn ready(&mut self, event_loop: &mut EventLoop<Server>, token: Token, events: EventSet) {
        if token == SERVER {
            if events.is_readable() {
                let token = self.queries.insert_with(|token| Query::new(token).unwrap()).unwrap();

                let mut query = &mut self.queries[token];
                query.wait_for_question();

                match self.socket.recv_from(&mut query.buffer) {
                    Ok(Some(remote_addr)) => {
                        {
                            let buf = &query.buffer;
                            let msg = Parser::parse(buf.bytes()).unwrap();
                        }
                        query.client_addr = Some(remote_addr);
                        query.question_upstream().ok().expect("error in questioning upstream");
                        query.register(event_loop).unwrap();
                    },
                    Ok(None) => {
                    }
                    Err(e) => {
                        println!("caught receive error {:?}", e);
                    }
                }
            }

            if events.is_writable() {
                if let Some(t) = self.write_queue.pop_front() {
                    let dest = self.queries[t].client_addr.expect("client_addr is None");
                    self.socket.send_to(&mut self.queries[t].buffer, &dest).ok().expect("error in socket send");
                    //self.queries[t].close(event_loop).ok().expect("socket failed to close");
                    self.queries.remove(t);

                    if self.write_queue.is_empty() {
                        event_loop.reregister(&self.socket, SERVER, EventSet::readable(),
                                              PollOpt::level()).ok().expect("register to only readable failed");
                    }
                }
            }
        } else {
            match self.queries[token].ready(event_loop, events) {
                Ok(_) => {
                },
                Err(e) => {
                    self.queries[token].close(event_loop).unwrap();
                    self.queries.remove(token);
                    println!("caught {:?} for token: {:?}", e, token);
                }
            }

            if self.queries[token].is_answer_ready() {
                self.write_queue.push_back(token);
                event_loop.reregister(&self.socket, SERVER, EventSet::readable()
                                      | EventSet::writable(),
                                      PollOpt::level()).ok().expect("register to writable failed");
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

        let _ = thread::spawn(move || {
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
