use mio::udp::*;
use mio::util::*;
use mio::{Token, EventLoop, EventSet, Handler, PollOpt};
use std::net::SocketAddr;
use query::*;
use datagram::*;
use std::collections::VecDeque;
use buf::*;

const SERVER: Token = Token(1);

#[derive (Debug)]
struct Server {
    socket: UdpSocket,
    datagrams: Slab<Datagram>,
    upstreams: Vec<SocketAddr>,
    queries: Slab<Query>,
    outgoing_queries: VecDeque<Token>,
    buffer: ByteBuf
}

const DATAGRAM_BUF_SIZE: usize = 65535;
const NUM_CONCURRENT_QUERIES: usize = 65535;

impl Server {
    fn new(s: UdpSocket) -> Server {
        Server{
            socket: s,
            datagrams: Slab::new_starting_at(Token(2), DATAGRAM_BUF_SIZE),
            queries: Slab::new_starting_at(Token(0), NUM_CONCURRENT_QUERIES),
            buffer: ByteBuf::new(),
            upstreams: vec!["8.8.8.8:53".parse().unwrap(), "8.8.4.4:53".parse().unwrap()],
            outgoing_queries: VecDeque::with_capacity(NUM_CONCURRENT_QUERIES)
        }
    }

    fn datagram_event(&mut self, t: Token, event_loop: &mut EventLoop<Server>, events: EventSet) -> Result<(), Error> {
        let datagram = &mut self.datagrams[token];
        let query = self.queries[datagram.query_token()]; // TODO: validation

        use datagram::EventResponse::*;

        match datagram.event(events) {
            Tx(Some(size)) => {
                // we sent something, so ask the query if we can transition to waiting
                assert!(size > 0);
                if self.queries[query_token].transmit_done(t, size) {
                    self.datagrams[t].set_rx();
                }
                if self.queries[query_token].is_done() {
                    self.datagrams[t].set_idle();
                }
            },
            Rx(Some((size, addr))) => {
                if self.queries[query_token].recv_done(t, addr, self.datagrams[t].get_ref()) {
                    self.queries[query_token].copy_message_bytes(self.datagrams[t].get_ref());
                    self.datagrams[t].set_idle();
                    self.datagrams[t].reregister(event_loop);

                    self.outgoing_queries.push_back(query_token);
                }
            }
            _ => {
            }
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
                let query_tok = self.queries.insert(Query::new()).unwrap();

                match self.queries[query_tok].rx(&self.socket) {
                    Ok(Some(())) => {
                        let query = &mut self.queries[query_tok];
                        for upstream in self.upstreams.iter() {
                            // get a datagram for outgoing
                            let token = self.datagrams.insert_with(|token| Datagram::new(token, query_tok, upstream.clone())).unwrap();
                            // link the query to the token
                            query.add_upstream_token(token);
                            // give it the correct bytes FIXME: a copy
                            self.datagrams[token].fill(query.question_bytes());
                            // register this datagram with the write event
                            self.datagrams[token].register(event_loop);
                        }
                        // so now N upstream requests have been registered for write for client 'remote_addr'
                    },
                    Ok(None) => {
                        self.queries.remove(query_tok);
                        println!("no data?");
                        // no data derp
                    },
                    Err(e) => {
                        self.queries.remove(query_tok);
                        // dunnolol
                        println!("argh {:?}", e);
                    }
                }
            }
            if events.is_writable() {
                let qt = *self.outgoing_queries.front().unwrap();

                let answer_bytes = self.queries[qt].question_bytes();

                match self.socket.send_to(answer_bytes, &self.queries[qt].get_addr().unwrap()) {
                    Ok(Some(size)) => {
                        if size == answer_bytes.len() {
                            self.outgoing_queries.pop_front().unwrap();
                        }
                    },
                    Ok(None) => {},
                    Err(e) => {
                        println!("caught error: {:?}", e);
                    }
                }
            }
        } else {
            // these are a query's datagram tx/r
            if let Err(e) = self.datagram_event(token, event_loop) {
                println!("caught error: {:?}", e);
                self.datagrams[token].set_idle();
                self.datagrams[token].reregister(event_loop);
                self.datagrams.remove(token);
            }
        }

        if !self.outgoing_queries.is_empty() {
            event_loop.register(&self.socket, SERVER, EventSet::readable() | EventSet::writable(), PollOpt::level() | PollOpt::edge()).unwrap();
        } else {
            event_loop.register(&self.socket, SERVER, EventSet::readable(), PollOpt::level() | PollOpt::edge()).unwrap();
        }
    }
}

pub fn run_server(s: UdpSocket) {
    let mut evt_loop = EventLoop::new().ok().expect("event loop failed");

    evt_loop.register(&s, SERVER, EventSet::readable(), PollOpt::level()| PollOpt::edge())
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
