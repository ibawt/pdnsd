#![allow(dead_code)]
#![allow(unused_variables)]
extern crate libc;
extern crate getopts;
extern crate mio;
extern crate bytes;
extern crate byteorder;
extern crate arrayvec;
extern crate smallvec;

mod errors;
mod dns;
mod buf;
mod datagram;
mod cache;
use getopts::{Matches, Options};
use std::env;
use libc::{setuid, setgid, fork, setsid};
mod users;
use users::get_ids;
use getopts::Fail;
use mio::udp::*;
mod query;
mod server;

fn drop_priv(args: &Matches) -> Result<(), &'static str> {
    let (user, group) = match (args.opt_str("user"), args.opt_str("group")) {
        (Some(u), Some(g)) => (u,g),
        _ => return Ok(())
    };

    unsafe {
        if let Ok((u,g)) = get_ids(&user, &group) {
            let gid = setgid(g as u32);
            if gid < 0 {
                return Err("gid")
            }
            let uid = setuid(u as u32);
            if uid < 0 {
                return Err("uid")
            }
        }
        else {
            return Err("unkown user or group");
        }
    }
    Ok(())
}

fn detach() -> bool {
    unsafe {
        let pid = fork();

        if pid < 0 {
            panic!("pid < 0");
        }

        if pid > 0 {
            return true
        }

        let sid = setsid();

        if sid < 0 {
            panic!("sid < 0");
        }
    }
    false
}

fn parse_opts() -> Result<Matches, Fail> {
    let mut opts = Options::new();

    opts.optflag("d", "daemonize", "run this in the background");
    opts.optopt("u", "user", "user to become", "USER");
    opts.optopt("g", "group", "group to become", "GROUP");
    opts.optflag("h", "help", "print this help menu");

    let matches = try!(opts.parse(env::args()));

    match matches.opt_present("h") {
        true => {
            print!("{}", opts.usage("Usage: [options]"));
            std::process::exit(0);
        },
        _ => Ok(matches)
    }

}

pub fn main() {
    let args = parse_opts().ok().expect("option parsing error!");

    if args.opt_present("daemonize") && detach() {
        return;
    }

    let addr = "127.0.0.1:9000".parse().unwrap();

    println!("Listening on {}", addr);

    let server = UdpSocket::bound(&addr).unwrap();

    if let Err(_) = drop_priv(&args) {
        panic!("Can't drop privileges exiting...");
    }
    server::run_server(server);
}
