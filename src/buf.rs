use std::{cmp};
use std::fmt;
use std::io;
use std::io::Write;
// stolen from bytebuf
/*
 *
 * ===== ByteBuf =====
 *
 */

/// A `Buf` backed by a contiguous region of memory.
///
/// This `Buf` is better suited for cases where there is a clear delineation
/// between reading and writing.

const BUF_LEN: usize = 512;

#[derive (Debug, PartialEq, Clone, Copy)]
enum Mode {
    Reading,
    Writing,
    Idle
}

pub struct ByteBuf {
    mode: Mode,
    pos: i32,
    lim: i32,
    mark: Option<i32>,
    mem: [u8; BUF_LEN]
}

impl fmt::Debug for ByteBuf {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "ByteBuf[pos: {}, lim: {}, mark: {:?}]", self.pos,
               self.lim, self.mark)
    }
}

impl ByteBuf {
    pub fn new() -> ByteBuf {
        ByteBuf {
            mode: Mode::Reading,
            mem: [0 ; BUF_LEN],
            pos: 0,
            lim: BUF_LEN as i32,
            mark: None
        }
    }

    pub fn get_mode(&self) -> Mode {
        self.mode
    }

    pub fn set_writable(&mut self) {
        self.clear();
        self.mode = Mode::Writing;
    }

    pub fn set_readable(&mut self) {
        self.mode = Mode::Reading;
    }

    pub fn clear(&mut self) {
        self.pos = 0;
        self.lim = BUF_LEN as i32;
    }

    pub fn capacity(&self) -> usize {
        BUF_LEN
    }

    pub fn flip(&mut self) {
        if self.mode == Mode::Writing {
            self.lim = self.pos;
            self.pos = 0;
            self.mode = Mode::Reading;
        } else {
            self.clear();
            self.mode = Mode::Writing;
        }
    }

    pub fn set_pos(&mut self, i: i32) {
        self.pos = i;
    }

    /// Flips the buffer back to mutable, resetting the write position
    /// to the byte after the previous write.
    pub fn resume(mut self) {
        self.pos = self.lim;
        self.lim = BUF_LEN as i32;
        self.mode = Mode::Writing;
    }

    /// Marks the current read location.
    ///
    /// Together with `reset`, this can be used to read from a section of the
    /// buffer multiple times. The marked location will be cleared when the
    /// buffer is flipped.
    pub fn mark(&mut self) {
        self.mark = Some(self.pos);
    }

    /// Resets the read position to the previously marked position.
    ///
    /// Together with `mark`, this can be used to read from a section of the
    /// buffer multiple times.
    ///
    /// # Panics
    ///
    /// This method will panic if no mark has been set.
    pub fn reset(&mut self) {
        self.pos = self.mark.take().expect("no mark set");
    }

    fn pos(&self) -> usize {
        self.pos as usize
    }

    fn lim(&self) -> usize {
        self.lim as usize
    }

    fn remaining(&self) -> i32 {
        self.lim - self.pos
    }

    pub fn bytes(&self) -> &[u8] {
        if self.mode == Mode::Reading {
            &self.mem[self.pos()..self.lim()]
        }
        else {
            &self.mem[..self.pos()]
        }
    }

    pub fn advance(&mut self, mut cnt: usize) {
        cnt = cmp::min(cnt, self.remaining() as usize);
        self.pos += cnt as i32;
    }

    pub fn mut_bytes(&mut self) -> &mut [u8] {
        assert!(self.mode == Mode::Writing);
        let pos = self.pos();
        let lim = self.lim();
        &mut self.mem[pos..lim]
    }
}

impl Write for ByteBuf {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let len = cmp::min(self.remaining() as usize, buf.len());
        {
            let mut b = self.mut_bytes();
            for i in 0..len {
                b[i] = buf[i];
            }
        }
        self.advance(len);

        Ok(len)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
