use byteorder::{BigEndian, ReadBytesExt};
use std::io::{Cursor};
use byteorder;

#[derive (Debug, PartialEq, Copy, Clone)]
#[allow(non_camel_case_types)]
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
pub const TTL_MAX: i32         = 1 << 31;
pub const UDP_MAX_LENGTH: u16  = 512;

impl QuestionClass {
    fn new(i: u16) -> Option<QuestionClass> {
        match i {
            0x01 => Some(QuestionClass::IN),
            _ => None
        }
    }
}

#[derive (Debug, Clone, PartialEq)]
pub struct Question {
    q_name: String,
    q_type: QuestionType,
    q_class: QuestionClass
}

#[derive (Debug, Clone)]
pub struct ResourceRecord {
    r_name: String,
    r_type: u16,
    r_class: u16,
    r_ttl: u32,
}

pub struct ResourceCname(String);


#[derive (Debug)]
pub struct Message {
    tx_id: u16,
    flags: u16,
    questions: Vec<Question>
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

#[derive (Debug)]
pub struct Parser<'a> {
    bytes: &'a [u8],
    cursor: Cursor<&'a [u8]>
}

impl Message {
    fn new(tx_id: u16, flags: u16, questions: Vec<Question>) -> Message {
        Message{
            tx_id: tx_id,
            flags: flags,
            questions: questions
        }
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
       (self.flags & 0b01111000_00000000) >> 3
    }
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

    fn parse_encoded_string(&mut self) -> Result<String, Error> {
        let mut s = String::with_capacity(NAMES_MAX_LENGTH);
        loop {
            let c = try!(self.cursor.read_u8());
            if c == 0 {
                break
            } else {
                if !s.is_empty() {
                    s.push('.');
                }
                else if s.len() > NAMES_MAX_LENGTH {
                    return Err(Error::Parse)
                }
                for _ in 0..c {
                    s.push(try!(self.cursor.read_u8()) as char);
                }
            }
        }
        Ok(s)
    }

    fn parse_question(&mut self) -> Result<Question,Error> {
        let s = try!(self.parse_encoded_string());
        let q_type = try!(QuestionType::new(try!(self.read_u16())).ok_or(Error::Parse));
        let q_class = try!(QuestionClass::new(try!(self.read_u16())).ok_or(Error::Parse));

        Ok(Question {
            q_name: s,
            q_type: q_type,
            q_class: q_class
        })
    }

    fn parse_resource_record(&mut self) -> Result<ResourceRecord, Error> {
        let s = try!(self.parse_encoded_string());
        let t = try!(self.parse_u16());
        let class = try!(self.read_u16());
        let ttl = try!(self.cursor.read_i32::<BigEndian>());

        let rd_len = try!(self.read_u16());

        let rdata = vec![];
        for _ in 0..rd_len {
            match t {
                0x01 => {
                    let addr = try!(p.read_u32::<BigEndian>());
                    println!("addr: {:x}", addr);
                }
            }
        }
    }

    pub fn parse(b: &[u8]) -> Result<Message,Error> {
        let mut p = Parser::new(b);
        let txn_id = try!(p.read_u16());
        let flags = try!(p.read_u16());

        let query_count = try!(p.read_u16());
        let an_count = try!(p.read_u16());
        let ns_count = try!(p.read_u16());
        let ar_count = try!(p.read_u16());

        let queries: Vec<Question> = try!((0..query_count).map(|_| p.parse_question()).collect());

        let answers: Vec<ResourceRecord> = try!((0..an_count).map(|_| p.parse_resource_record()).collect());

        println!("an_count: {}", an_count);

        Ok(Message::new(txn_id, flags, queries))
    }
}

#[cfg(test)]
mod tests {
    use self::super::*;

    #[test]
    fn simple_query() {
        let bytes = include_bytes!("../test/dns_request.bin");

        let msg = Parser::parse(bytes).unwrap();
        println!("flags: {:x}", msg.flags);
        assert_eq!(true, msg.is_query());
        assert_eq!(0, msg.opcode());
        assert_eq!(true, msg.recursion_desired());
        assert_eq!(false, msg.recursion_available());
        assert_eq!(0, msg.return_code());

        assert_eq!("fark.com", msg.questions[0].q_name);
        assert_eq!(QuestionType::HOST_RECORD, msg.questions[0].q_type);
        assert_eq!(QuestionClass::IN, msg.questions[0].q_class);
    }

    #[test]
    fn simple_response() {
        let bytes = include_bytes!("../test/dns_response.bin");

        let msg = Parser::parse(bytes).unwrap();
        println!("parser: {:?}", msg);

        assert!(msg.is_response());

        assert!(false);
    }
}
