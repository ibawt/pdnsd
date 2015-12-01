use mio::udp::*;
use mio::util::*;
use mio::{Token, EventLoop, EventSet, Handler, PollOpt};
use std::net::SocketAddr;
use query::*;
use dns::Message;
use datagram::*;
use std::collections::VecDeque;

const SERVER: Token = Token(1);

#[derive (Debug)]
struct Server {
    socket: UdpSocket,
    datagrams: Slab<Datagram>,
    upstreams: Vec<SocketAddr>,
    queries: Slab<Query>,
    outgoing_queries: VecDeque<Token>,
    buffer: Vec<u8>
}

const DATAGRAM_BUF_SIZE: usize = 256;
const NUM_CONCURRENT_QUERIES: usize = 256;

impl Server {
    fn new(s: UdpSocket) -> Server {
        Server{
            socket: s,
            datagrams: Slab::new_starting_at(Token(2), DATAGRAM_BUF_SIZE),
            queries: Slab::new_starting_at(Token(0), NUM_CONCURRENT_QUERIES),
            buffer: vec![],
            upstreams: vec!["8.8.8.8:53".parse().unwrap(), "8.8.4.4:53".parse().unwrap()],
            outgoing_queries: VecDeque::new()
        }
    }

    fn handle_datagram_response(&mut self, t: Token, de: DatagramEventResponse, event_loop: &mut EventLoop<Server>) {
        // get my query object
        let query_token = self.datagrams[t].query_token();

        match de {
            DatagramEventResponse::Transmit(Some(size)) => {
                // we sent something, so ask the query if we can transition to waiting
                println!("tx - {} bytes", size);
                assert!(size > 0);
                if self.queries[query_token].transmit_done(t, size) {
                    println!("should transition into rx'ing");
                    self.datagrams[t].set_rx();
                }
                if self.queries[query_token].is_done() {
                    println!("marking done?");
                    self.datagrams[t].set_idle();
                }
            },
            DatagramEventResponse::Recv(Some((size, addr))) => {
                println!("recv done {} bytes from {}", size, addr);
                if self.queries[query_token].recv_done(t, addr, self.datagrams[t].get_ref()) {
                    println!("sending rx back to client");
                    self.queries[query_token].copy_message_bytes(self.datagrams[t].get_ref());
                    self.datagrams[t].set_idle();
                    self.datagrams[t].reregister(event_loop);

                    self.outgoing_queries.push_back(query_token);
                }
            }
            _ => {
            }
        }
        self.datagrams[t].reregister(event_loop);
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
                println!("in here?");
                let mut buffer = [0u8 ; 512 ];

                match self.socket.recv_from(&mut buffer) {
                    Ok(Some((size, remote_addr))) => {
                        println!("rx: {} bytes from {}", size, remote_addr);
                        if size == 0 {
                            println!("0 bytes??");
                            return;
                        }
                        // State Machine Start
                        let message = Message::new(&buffer[0..size]).unwrap();
                        // read valid dns requesst
                        let query_tok = self.queries.insert_with(|token| Query::new(remote_addr, token, message)).unwrap();
                        // get a query
                        let mut query = &mut self.queries[query_tok];
                        query.copy_message_bytes(&buffer[0..size]);
                        // copy this into the quer for retransmit

                        println!("registering upstreams");
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
                        println!("no data?");
                        // no data derp
                    },
                    Err(e) => {
                        // dunnolol
                        println!("argh {:?}", e);
                    }
                }
            }
            if events.is_writable() {
                let qt = *self.outgoing_queries.front().unwrap();

                let answer_bytes = self.queries[qt].question_bytes();
                println!("sending back! {} bytes", answer_bytes.len());

                match self.socket.send_to(answer_bytes, &self.queries[qt].get_addr()) {
                    Ok(Some(size)) => {
                        if size == answer_bytes.len() {
                            self.outgoing_queries.pop_front().unwrap();
                            println!("we should really clean up now :(");
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
            match self.datagrams[token].event(events) {
                Ok(result) => {
                    // we either fully sent something or read something to handle it
                    self.handle_datagram_response(token, result, event_loop);
                },
                Err(e) => {
                    println!("caught error: {:?}", e);
                }
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
