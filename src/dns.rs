use byteorder::{BigEndian, ReadBytesExt};
use std::io::{Cursor};
use byteorder;
use std::net::Ipv4Addr;
use arrayvec::*;
use std::borrow::Cow;
use smallvec::SmallVec;

#[derive (Debug, PartialEq, Copy, Clone)]
#[allow(non_camel_case_types, dead_code)]
pub enum QuestionType {
    A           = 0x01,
    NS          = 0x02,
    MD          = 0x03, // obsolete
    MF          = 0x04, // obsolete
    CNAME       = 0x05,
    SOA         = 0x06,
    MB          = 0x07,
    MG          = 0x08,
    MR          = 0x09,
    NULL        = 0x0a,
    WKS         = 0x0b,
    PTR         = 0x0c,
    HINFO       = 0x0d,
    MINFO       = 0x0e,
    MX          = 0x0f,
    TXT         = 0x10,
    // only valid for questions
    SRV         = 0x21,
    IXFR        = 0xfb,
    AXFR        = 0xfc,
    ALL         = 0xff
}

impl QuestionType {
    fn new(i: u16) -> Option<QuestionType> {
        use self::QuestionType::*;
        Some(match i {
            0x01 => A,
            0x02 => NS,
            0x05 => CNAME,
            0x0c => PTR,
            0x0f => MX,
            0x21 => SRV,
            0xfb => IXFR,
            0xfc => AXFR,
            0xff => ALL,
            _ => return None
        })
    }
}

#[derive (Debug, PartialEq, Copy, Clone)]
pub enum QuestionClass {
    IN = 0x01
}

pub const LABEL_MAX_LENGTH: usize = 63;
pub const NAMES_MAX_LENGTH: usize = 255;

impl QuestionClass {
    fn new(i: u16) -> Option<QuestionClass> {
        match i {
            0x01 => Some(QuestionClass::IN),
            _ => None
        }
    }
}

pub type Name = ArrayVec<[u8;256]>;

#[derive (Debug, Clone)]
pub struct Question {
    q_name: Name,
    q_type: QuestionType,
    q_class: QuestionClass
}

impl Question {
    pub fn name(&self) -> Cow<str> {
        String::from_utf8_lossy(&self.q_name)
    }
}

#[derive (Debug, Clone)]
pub struct ResourceRecord {
    pub r_name: Name,
    pub r_type: u16,
    pub r_class: u16,
    pub r_ttl: i32,
    pub r_data: ResourceData
}

impl ResourceRecord {
    pub fn name(&self) -> Cow<str> {
        String::from_utf8_lossy(&self.r_name)
    }
}

#[derive (Debug, Clone, PartialEq)]
pub enum ResourceData {
    A(Ipv4Addr),
    Bytes(Vec<u8>)
}

#[derive (Debug, Clone)]
pub struct Message {
    pub tx_id: u16,
    pub flags: u16,
    pub questions: SmallVec<[Question;2]>,
    pub answers: SmallVec<[ResourceRecord;8]>,
    pub name_server: Vec<ResourceRecord>,
    pub additional: Vec<ResourceRecord>
}

#[derive (Debug)]
pub enum Error {
    Byte(byteorder::Error),
    Parse
}

impl From<byteorder::Error> for Error {
    fn from(err: byteorder::Error) -> Error {
        Error::Byte(err)
    }
}

impl Message {
    pub fn default() -> Message {
        Message {
            tx_id: 0,
            flags: 0,
            questions: SmallVec::new(),
            answers: SmallVec::new(),
            name_server: vec![],
            additional: vec![]
        }
    }

    pub fn new(b: &[u8]) -> Result<Message, Error> {
        let mut m = Message::default();

        try!(m.parse(b));
        Ok(m)
    }

    pub fn parse(&mut self, b: &[u8]) -> Result<(), Error> {
        Parser::parse(self, b)
    }

    fn is_query(&self) -> bool {
        (self.flags & (1 << 15)) == 0
    }

    fn is_response(&self) -> bool {
        !self.is_query()
    }

    fn is_auth_answer(&self) -> bool {
        (self.flags & 0b0_0000_1_00_00000000) != 0
    }

    fn is_truncated(&self) -> bool {
        (self.flags & 0b0_0000_0_1_0_00000000) != 0
    }

    fn recursion_desired(&self) -> bool {
        (self.flags & 0b0_0000_0_0_1_00000000) != 0
    }

    fn recursion_available(&self) -> bool {
        (self.flags & 0b0000000010000000) != 0
    }

    fn return_code(&self) -> u16 {
        return self.flags & 0b1111
    }

    fn opcode(&self) -> u16 {
       (self.flags & 0b01111000_00000000) >> (3 + 8)
    }
}


#[derive (Debug)]
struct Parser<'a> {
    bytes: &'a [u8],
    cursor: Cursor<&'a [u8]>
}

impl<'a> Parser<'a> {
    pub fn new(b: &'a [u8]) -> Parser<'a> {
        Parser{
            bytes: b,
            cursor: Cursor::new(b)
        }
    }

    fn read_u16(&mut self) -> byteorder::Result<u16> {
        self.cursor.read_u16::<BigEndian>()
    }

    fn peek_u8(&self) -> Option<u8> {
        if (self.cursor.position() as usize) >= self.bytes.len() {
            None
        } else {
            Some(self.bytes[self.cursor.position() as usize])
        }
    }

    fn parse_encoded_string(&mut self, s: &mut Name) -> Result<(), Error> {
        const OFFSET_MASK: u8 = 0b1100_0000;

        while let Some(c) = self.peek_u8() {
            if c == 0 {
                try!(self.cursor.read_u8());
                break
            }
            else if (c & OFFSET_MASK) == OFFSET_MASK { //10 & 01 are invalid
                let offset = try!(self.read_u16()) & !((OFFSET_MASK as u16) << 8);
                let pos = self.cursor.position();

                assert!((offset as usize) < self.bytes.len());

                self.cursor.set_position(offset as u64);
                loop {
                    if let Some(cc) = self.peek_u8() {
                        if cc == 0 {
                            try!(self.cursor.read_u8());
                            break
                        } else {
                            try!(self.read_label(s));
                        }
                    } else {
                        return Err(Error::Parse)
                    }
                }
                self.cursor.set_position(pos);
                return Ok(())
            } else {
                try!(self.read_label(s));
            }
        }
        Ok(())
    }


    fn read_label(&mut self, s: &mut Name) -> Result<(), Error> {
        let c = try!(self.cursor.read_u8());
        assert!(c != 0);

        if (c as usize) < LABEL_MAX_LENGTH {
            if !s.is_empty() {
                s.push(b'.');
            }
            for _ in 0..c {
                s.push(try!(self.cursor.read_u8()));
            }
        } else {
            return Err(Error::Parse)
        }
        Ok(())
    }

    fn parse_question(&mut self) -> Result<Question,Error> {
        let mut name = Name::new();
        try!(self.parse_encoded_string(&mut name));
        let q_type = try!(QuestionType::new(try!(self.read_u16())).ok_or(Error::Parse));
        let q_class = try!(QuestionClass::new(try!(self.read_u16())).ok_or(Error::Parse));

        Ok(Question {
            q_name: name,
            q_type: q_type,
            q_class: q_class
        })
    }

    fn read_ipv4(&mut self) -> Result<Ipv4Addr, Error> {
        let a = try!(self.cursor.read_u8());
        let b = try!(self.cursor.read_u8());
        let c = try!(self.cursor.read_u8());
        let d = try!(self.cursor.read_u8());

        Ok(Ipv4Addr::new(a, b, c, d))
    }

    fn read_bytes(&mut self, len: u16) -> Result<Vec<u8>, Error> {
        let mut v = Vec::with_capacity(len as usize);

        for _ in 0..len {
            v.push(try!(self.cursor.read_u8()));
        }

        Ok(v)
    }

    fn parse_resource_record(&mut self) -> Result<ResourceRecord, Error> {
        let mut name = Name::new();
        try!(self.parse_encoded_string(&mut name));
        let t = try!(self.read_u16());
        let class = try!(self.read_u16());
        let ttl = try!(self.cursor.read_i32::<BigEndian>());
        let rd_len = try!(self.read_u16());

        let rdata = match t {
            0x01 => ResourceData::A(try!(self.read_ipv4())),
            _ => ResourceData::Bytes(try!(self.read_bytes(rd_len)))
        };

        Ok(ResourceRecord{
            r_name: name,
            r_type: t,
            r_class: class,
            r_ttl: ttl,
            r_data: rdata
        })
    }

    pub fn parse(m: &mut Message, b: &[u8]) -> Result<(),Error> {
        let mut p = Parser::new(b);
        let txn_id = try!(p.read_u16());
        let flags = try!(p.read_u16());

        let query_count = try!(p.read_u16());
        let an_count = try!(p.read_u16());
        let ns_count = try!(p.read_u16());
        let ar_count = try!(p.read_u16());

        for _ in 0..query_count {
            m.questions.push(try!(p.parse_question()));
        }

        for _ in 0..an_count {
            m.answers.push(try!(p.parse_resource_record()));
        }

        m.tx_id = txn_id;
        m.flags = flags;

        Ok(())
    }
}

pub fn parse_txn_id(bytes: &[u8]) -> Option<u16> {
    if bytes.len() < 2 {
        return None
    }
    let mut c = Cursor::new(bytes);

    c.read_u16::<BigEndian>().ok()
}

#[cfg(test)]
mod tests {
    use self::super::*;
    use std::net::Ipv4Addr;

    #[test]
    fn simple_query() {
        let bytes = include_bytes!("../test/dns_request.bin");

        let msg = Message::new(bytes).unwrap();
        assert_eq!(true, msg.is_query());
        assert_eq!(0, msg.opcode());
        assert_eq!(true, msg.recursion_desired());
        assert_eq!(false, msg.recursion_available());
        assert_eq!(0, msg.return_code());

        assert_eq!("fark.com", msg.questions[0].name());
        assert_eq!(QuestionType::A, msg.questions[0].q_type);
        assert_eq!(QuestionClass::IN, msg.questions[0].q_class);
    }

    #[test]
    fn simple_response() {
        let bytes = include_bytes!("../test/dns_response.bin");

        let msg = Message::new(bytes).unwrap();

        assert!(msg.is_response());
        assert_eq!(1, msg.answers.len());
        assert_eq!("fark.com", msg.answers[0].name());
        assert_eq!(1, msg.answers[0].r_type);
        assert_eq!(1, msg.answers[0].r_class);
        assert_eq!(ResourceData::A(Ipv4Addr::new(64,191,171,200)), msg.answers[0].r_data);
    }

    #[test]
    fn multi_request() {
        let bytes = include_bytes!("../test/multi_a_request.bin");

        let msg = Message::new(bytes).unwrap();

        assert!(msg.is_query());
        assert_eq!(0, msg.opcode());
        assert_eq!(0, msg.return_code());
        assert_eq!("shops.shopify.com", msg.questions[0].name());
    }

    #[test]
    fn multi_response() {
        let bytes = include_bytes!("../test/multi_a_response.bin");

        let msg = Message::new(bytes).unwrap();

        assert!(msg.is_response());
        assert_eq!(0, msg.opcode());
        assert_eq!(0, msg.return_code());

        assert_eq!(4, msg.answers.len());

        for answer in msg.answers.iter() {
            assert_eq!(1, answer.r_type);
            assert_eq!(1, answer.r_class);
            assert_eq!("shops.shopify.com", answer.name());
        }
        assert_eq!(ResourceData::A(Ipv4Addr::new(23,227,38,71)), msg.answers[0].r_data);
        assert_eq!(ResourceData::A(Ipv4Addr::new(23,227,38,70)), msg.answers[1].r_data);
        assert_eq!(ResourceData::A(Ipv4Addr::new(23,227,38,69)), msg.answers[2].r_data);
        assert_eq!(ResourceData::A(Ipv4Addr::new(23,227,38,68)), msg.answers[3].r_data);
    }
}
