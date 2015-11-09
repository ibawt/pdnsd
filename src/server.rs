use mio::udp::*;
use mio::util::*;
use mio::{Token, EventLoop, EventSet, Handler, PollOpt};
use std::net::SocketAddr;
use std::io;
use std::net;
use query::*;
use dns::Message;
use std::io::*;

const SERVER: Token = Token(1);

#[derive (Debug, PartialEq)]
enum Mode {
    Reading,
    Writing,
    Idle
}

#[derive (Debug)]
struct Datagram {
    token: Token,
    query_token: Token,
    socket_addr: SocketAddr,
    socket: UdpSocket,
    buf: Cursor<Vec<u8>>,
    mode: Mode
}

impl Datagram {
    fn new(t: Token, qt: Token, remote: SocketAddr) -> Datagram {
        Datagram{
            token: t,
            query_token: qt,
            socket_addr: remote,
            socket: UdpSocket::v4().unwrap(),
            buf: Cursor::new(Vec::with_capacity(512)),
            mode: Mode::Reading
        }
    }

    fn fill(&mut self, bytes: &[u8]) {
        self.buf.set_position(0);
        self.buf.write_all(bytes).unwrap();
        self.mode = Mode::Writing;
    }

    fn reset(&mut self) {
        self.buf.set_position(0);
    }

    fn get_ref(&self) -> &[u8] {
        self.buf.get_ref()
    }

    fn transmit(&mut self) {
        match self.socket.send_to(self.buf.get_ref(), &self.socket_addr) {
            Ok(Some(size)) => {
                println!("size = {}", size);
            },
            Ok(None) => {
                println!("none");
            }
            Err(e) => {
                println!("err = {:?}", e);
            }
        }
    }

    fn recv(&mut self) {
        match self.socket.recv_from(self.buf.get_mut()) {
            Ok(Some(_)) => {
            }
            Ok(None) => {
                println!("none");
            },
            Err(e) => println!("err = {:?}", e)
        }
    }

    fn event(&mut self, events: EventSet) -> io::Result<Option<()>> {
        match self.mode {
            Mode::Writing => {
                if events.is_writable() {
                    self.transmit();
                    return Ok(Some(()))
                }
            },
            Mode::Reading => {
                if events.is_readable() {
                    self.recv();
                    return Ok((Some(())))
                }
            },
            _ => {}
        }
        Ok(None)
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

#[derive (Debug)]
struct Server {
    socket: UdpSocket,
    datagrams: Slab<Datagram>,
    upstreams: Vec<SocketAddr>,
    queries: Slab<Query>,
    buffer: Vec<u8>
}

const DATAGRAM_BUF_SIZE: usize = 65536;
const NUM_CONCURRENT_QUERIES: usize = 65536;

impl Server {
    fn new(s: UdpSocket) -> Server {
        Server{
            socket: s,
            datagrams: Slab::new_starting_at(Token(2), DATAGRAM_BUF_SIZE),
            queries: Slab::new_starting_at(Token(0), NUM_CONCURRENT_QUERIES),
            buffer: vec![],
            upstreams: vec!["8.8.8.8:53".parse().unwrap(), "8.8.4.4:53".parse().unwrap()]
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
                self.buffer.clear();
                self.buffer.reserve(512);

                match self.socket.recv_from(&mut self.buffer) {
                    Ok(Some((size, remote_addr))) => {
                        let message = Message::new(&self.buffer[0..size]).unwrap();
                        let query_tok = self.queries.insert_with(|token| Query::new(remote_addr, token, message)).unwrap();
                        let mut query = &mut self.queries[query_tok];
                        query.copy_message_bytes(&self.buffer);

                        for upstream in self.upstreams.iter() {
                            let token = self.datagrams.insert_with(|token| Datagram::new(token, query_tok, upstream.clone())).unwrap();
                            query.add_upstream_token(token);
                            self.datagrams[token].fill(query.question_bytes());
                            // self.datagrams[token].start(event_loop);
                        }
                    },
                    Ok(None) => {},
                    Err(e) => {
                        println!("argh {:?}", e);
                    }
                }
            }
        } else {
            match self.datagrams[token].event(events) {
                Ok(None) => {},
                Ok(Some(())) => {
                    //self.queries[query_token].datagram_done(token, result);
                    self.datagrams.remove(token);
                },
                Err(e) => {
                    println!("caught {:?} for token: {:?}", e, token);
                }
            }
        }
    }
}

pub fn run_server(s: UdpSocket) {
    let mut evt_loop = EventLoop::new().ok().expect("event loop failed");

    evt_loop.register(&s, SERVER, EventSet::readable(), PollOpt::level())
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

        let msg = Message::new(&response).unwrap();

        let _ = thread::spawn(move || {
            let server_addr = "0.0.0.0:9080".parse().unwrap();
            let s = mio::udp::UdpSocket::bound(&server_addr).unwrap();
            run_server(s);
        });


        let t = thread::spawn(move || {
            let addr: SocketAddr = "0.0.0.0:9080".parse().unwrap();
            let request = include_bytes!("../test/dns_request.bin");
            let output = test_dns_request(request, &addr);
            let msg = Message::new(&output).unwrap();

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
