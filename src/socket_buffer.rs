use buf::{ByteBuf, MutByteBuf};
use mio::{EventSet};

#[derive (Debug, PartialEq, Clone, Copy)]
enum SocketState {
    Writing,
    Reading,
    Closed
}

#[derive (Debug)]
pub struct Buffer {
    state: SocketState,
    buf: Option<ByteBuf>,
    mut_buf: Option<MutByteBuf>
}
impl Buffer {
    pub fn new() -> Buffer {
        Buffer{
            state: SocketState::Closed,
            buf: None,
            mut_buf: None
        }
    }

    pub fn buf(& mut self) -> &mut ByteBuf {
        match self.state {
            SocketState::Writing => {
                match self.buf {
                    Some(ref mut b) => b,
                    _ => panic!("invalid buffer state")

                }
            }
            _ => panic!("invalid socket state")
        }
    }

    pub fn mut_buf(&mut self) -> &mut MutByteBuf {
        match self.state {
            SocketState::Reading => {
                match self.mut_buf {
                    Some(ref mut b) => b,
                    _ => panic!("invalid buffer state")

                }
            }
            _ => panic!("invalid socket state")
        }
    }

    fn is_mut_buf(&self) -> bool {
        match self.state {
            SocketState::Reading => true,
            _ => false
        }
    }

    fn is_buf(&self) -> bool {
        match self.state {
            SocketState::Writing => true,
            _ => false
        }
    }

    fn flip(&mut self) {
        match self.state {
            SocketState::Reading => self.to_buf(),
            SocketState::Writing => self.to_mut_buf(),
            _ => panic!("can't flip when state is Closed")
        }
    }

    fn to_buf(&mut self) {
        match self.state {
            SocketState::Closed => {
                self.buf = Some(ByteBuf::new());
            },
            SocketState::Reading => {
                self.buf = self.mut_buf.take().map(|b| b.flip())
            },
            SocketState::Writing => ()
        }
    }

    fn to_mut_buf(&mut self) {
        match self.state {
            SocketState::Closed => {
                self.mut_buf = Some(ByteBuf::new().flip());
            },
            SocketState::Writing => {
                self.mut_buf = self.buf.take().map(|b| b.flip())
            },
            SocketState::Reading => {
                //noop
            }
        }
    }

    pub fn to_writing(&mut self) {
        match self.state {
            SocketState::Reading | SocketState::Closed => {
                self.to_mut_buf();
                self.state = SocketState::Writing;
            },
            _ => ()
        }
    }

    pub fn to_reading(&mut self) {
        match self.state {
            SocketState::Writing | SocketState::Closed => {
                self.to_buf();
                self.state = SocketState::Reading;
            },
            _ => ()
        }
    }

    pub fn close(&mut self) {
        self.buf = None;
        self.mut_buf = None;
        self.state = SocketState::Closed;
    }

    pub fn event_set(&self) -> EventSet {
        match self.state {
            SocketState::Closed => EventSet::none(),
            SocketState::Writing => EventSet::writable(),
            SocketState::Reading => EventSet::readable()
        }
    }
}
