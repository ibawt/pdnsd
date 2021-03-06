use mio::udp::*;
use errors::*;
use mio::util::*;
use mio::{Token, EventLoop, EventSet, Handler, PollOpt};
use std::net::SocketAddr;
use query::*;
use datagram::*;
use std::collections::VecDeque;
use std::thread;
use mio;
use chan;
use cache::*;

const SERVER: Token = Token(1);

#[derive (Debug)]
struct Server {
    cache: Cache,
    socket: UdpSocket,
    datagrams: Slab<Datagram>,
    upstreams: Vec<SocketAddr>,
    queries: Slab<Query>,
    outgoing_queries: VecDeque<Token>,
}

const NUM_CONCURRENT_QUERIES: usize = 256;
const DATAGRAM_BUF_SIZE: usize = NUM_CONCURRENT_QUERIES*2;

impl Server {
    fn new(s: UdpSocket) -> Server {
        Server{
            cache: Cache::new(),
            socket: s,
            datagrams: Slab::new_starting_at(Token(2), DATAGRAM_BUF_SIZE),
            queries: Slab::new_starting_at(Token(0), NUM_CONCURRENT_QUERIES),
            upstreams: vec!["8.8.8.8:53".parse().unwrap(), "8.8.4.4:53".parse().unwrap()],
            outgoing_queries: VecDeque::with_capacity(NUM_CONCURRENT_QUERIES)
        }
    }

    fn outgoing_query_event(&mut self) -> Result<Option<Token>, Error> {
        let qt = try!(self.outgoing_queries.front().ok_or("no outgoing queries!"));
        let answer_bytes = self.queries[*qt].question_bytes();

        if let Some(size) =  try!(self.socket.send_to(answer_bytes, try!(self.queries[*qt].get_addr().ok_or("no remote address")))) {
            if size == answer_bytes.len() {
                return Ok(Some(*qt))
            }
        }
        Ok(None)
    }

    fn destroy_query(&mut self, event_loop: &mut EventLoop<Server>, t: Token) -> Result<(), Error> {
        let query = &mut self.queries[t];

        for i in query.upstream_tokens() {
            self.datagrams[i].set_idle();
            try!(self.datagrams[i].reregister(event_loop));
            try!(event_loop.deregister(self.datagrams[i].socket()));
            self.datagrams.remove(i);
        }

        if let Some(timeout) = query.take_timeout() {
            event_loop.clear_timeout(timeout);
        }

        Ok(())
    }

    fn datagram_event(&mut self, token: Token, event_loop: &mut EventLoop<Server>, events: EventSet) -> Result<(), Error> {
        if !self.datagrams.contains(token) {
            // event in queue for a dead token
            return Ok(())
        }

        if !self.queries.contains(self.datagrams[token].query_token()) {
            warn!("event on dead query: [{:?}]", self.datagrams[token].query_token());
            return Ok(())
        }

        let qt = self.datagrams[token].query_token();

        let done = try!(self.queries[qt].datagram_event(&mut self.datagrams[token], events));

        if done {
            self.outgoing_queries.push_back(self.datagrams[token].query_token());
            return self.destroy_query(event_loop, qt)
        } else {
            try!(self.datagrams[token].reregister(event_loop))
        }
        Ok(())
    }
}

#[derive (Debug)]
pub enum ServerEvent {
    Quit
}

impl Handler for Server {
    type Timeout = Token;
    type Message = ServerEvent;

    fn notify(&mut self, event_loop: &mut EventLoop<Server>, msg: ServerEvent) {
        match msg {
            ServerEvent::Quit => {
                info!("Received quit event, shutting down event loop.");
                event_loop.shutdown();
            }
        }
    }

    fn timeout(&mut self, event_loop: &mut EventLoop<Server>, query_token: Token) {
        if !self.queries.contains(query_token) {
            warn!("timeout on dead token: {:?}", query_token);
            return;
        }

        info!("[{:?}] has timed out", query_token);

        if let Err(e) = self.destroy_query(event_loop, query_token) {
            warn!("error in destroy query: {:?}", e);
        }

        self.queries.remove(query_token);
    }

    fn interrupted(&mut self, event_loop: &mut EventLoop<Server>) {
        warn!("Interrupted shutting down...");
        event_loop.shutdown()
    }

    fn ready(&mut self, event_loop: &mut EventLoop<Server>, token: Token, events: EventSet) {
        if token == SERVER {

            if events.is_readable() {
                let query_tok = match self.queries.insert_with(|qt| Query::new(qt)) {
                    Some(t) => t,
                    None => {
                        error!("error in query insert");
                        return;
                    }
                };

                match self.queries[query_tok].rx(&self.socket) {
                    Ok(Some(())) => {
                        if self.queries[query_tok].answer_in_cache(&self.cache) {
                        } else {
                            let query = &mut self.queries[query_tok];
                            for upstream in self.upstreams.iter() {
                                // get a datagram for outgoing
                                let token = match self.datagrams.insert_with(|token| Datagram::new(token, query_tok, upstream.clone())) {
                                    Some(t) => t,
                                    None => {
                                        error!("error in datagram insert");
                                        //self.queries.remove(query_tok);
                                        return;
                                    }
                                };
                                // link the query to the token
                                query.add_upstream_token(token);
                                // give it the correct bytes FIXME: a copy
                                if let Err(e) = self.datagrams[token].fill(query.question_bytes()) {
                                    error!("datagram [{:?}] error in fill: {:?}", token, e);
                                    return;
                                }
                                // register this datagram with the write event
                                if let Err(e) = self.datagrams[token].register(event_loop) {
                                    error!("datagram [{:?}] error in register: {:?}", token, e);
                                }
                            }

                            let timeout = event_loop.timeout_ms(query_tok, 10 * 1000).unwrap();

                            query.set_timeout(timeout);
                        }
                    },
                    Ok(None) => {
                        self.queries.remove(query_tok);
                        error!("no data?");
                        // no data derp
                    },
                    Err(e) => {
                        self.queries.remove(query_tok);
                        // dunnolol
                        error!("argh {:?}", e);
                    }
                }
            }
            if events.is_writable() {
                match self.outgoing_query_event() {
                    Ok(Some(query_token)) => {
                        let qt = self.outgoing_queries.pop_front().expect("this shouldn't happen");
                        assert!(qt == query_token);
                        self.queries.remove(qt);
                    },
                    Ok(None) => (),
                    Err(e) => {
                        error!("outgoing query event: {:?}", e);
                    }
                }
            }
        } else {
            // these are a query's datagram tx/r
            if let Err(e) = self.datagram_event(token, event_loop, events) {
                error!("datagram event caught error: {:?}", e);
                //self.datagrams.remove(token);
            }
        }

        if self.outgoing_queries.is_empty() {
            if let Err(e) = event_loop.reregister(&self.socket, SERVER, EventSet::readable(), PollOpt::level() | PollOpt::edge()) {
               error!("listening socket rx reregister failed: {:?}", e); 
            }
        } else {
            if let Err(e) = event_loop.reregister(&self.socket, SERVER, EventSet::readable() | EventSet::writable(), PollOpt::level() | PollOpt::edge()) {
               error!("listening socket tx/rx reregister failed: {:?}", e); 
            }
        }
    }
}

pub fn run_server(s: UdpSocket) -> (thread::JoinHandle<()>, mio::Sender<ServerEvent>, chan::Receiver<i32>) {
    let mut evt_loop = EventLoop::new().ok().expect("event loop failed");

    evt_loop.register(&s, SERVER, EventSet::readable(), PollOpt::level()| PollOpt::edge())
        .ok().expect("registration failed");

    let (end_sender, rx) = chan::sync(0);

    let sender = evt_loop.channel();

    let thr = thread::spawn(move || {
        info!("EventLoop thread started!");
        evt_loop.run(&mut Server::new(s)).ok().expect("event loop run");
        info!("EventLoop thread ended!");
        chan_select! {
            default => {},
            end_sender.send(0) => {}
        }
    });

    (thr, sender, rx)
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
