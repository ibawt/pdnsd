use std::io::Cursor;
use byteorder::{BigEndian, ReadBytesExt}

#[derive (Debug, PartialEq, Copy, Clone)]
pub enum QuestionType {
    HOST_RECORD = 0x01,
    NAME_SERVER = 0x02,
    CNAME       = 0x05,
    PTR         = 0x0c,
    MX          = 0x0f,
    SRV         = 0x21,
    IXFR        = 0xfb,
    AXFR        = 0xfc,
    ALL         = 0xff
}

#[derive (Debug, PartialEq, Copy, Clone)]
pub enum QuestionClass {
    IN = 0x01
}

#[derive (Debug, Clone)]
pub struct DnsHeader {
    tx_id: u16,
    flags: u16,
}

#[derive (Debug, Copy, Clone)]
pub struct Question {
    name: String,
    type: u16,
    class: u16
}

#[derive (Debug, Clone)]
pub struct Answer {
    name: String,
    type: u16,
    class: u16,
    ttl: u32,
    res_data: Vector<ResourceData>
}

#[derive (Debug)]
pub struct ResourceData {
}

pub struct DnsMessage {
    packet: Vec<u8>,
}

impl DnsMessage {
    pub new(b: &[u8]) -> DnsMessage {
        DnsMessage{
            packet: b.iter().collect()
        }
    }

    pub transaction_id(&self) -> Result<u16, String> {
        let c = Cursor::new(self.packet);

        c.read_u16::<BigEndian>()
    }
}
