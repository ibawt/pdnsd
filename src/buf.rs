use bytes::traits::{Buf, MutBuf};
use std::{cmp};
use std::fmt;

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
pub enum Mode {
    Reading,
    Writing
}

pub struct ByteBuf {
    mem: [u8; BUF_LEN],
    mode: Mode,
    pos: i32,
    lim: i32,
    mark: Option<i32>,
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
        self.mode = Mode::Writing;
        self.clear();
    }

    pub fn set_readable(&mut self) {
        self.mode = Mode::Reading;
        self.clear();
    }

    pub fn clear(&mut self) {
        self.pos = 0;
        self.lim = BUF_LEN as i32;
    }

    // #[inline]
    // pub fn write_slice(&mut self, src: &[u8]) -> usize {
    //     let cnt = src.len() as i32;
    //     let rem = self.remaining_u32();

    //     if rem < cnt {
    //         self.write_ptr(src.as_ptr(), rem as u32)
    //     } else {
    //         self.write_ptr(src.as_ptr(), cnt as u32)
    //     }
    // }

    // #[inline]
    // fn write_ptr(&mut self, src: *const u8, len: u32) -> usize {
    //     unsafe {
    //         ptr::copy_nonoverlapping(
    //             src,
    //             self.mem.as_mut_ptr().offset(self.pos as isize),
    //             len as usize);

    //         self.pos += len as i32;
    //         len as usize
    //     }
    // }

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

    /// Flips the buffer back to mutable, resetting the write position
    /// to the byte after the previous write.
    pub fn resume(mut self) {
        self.pos = self.lim;
        self.lim = BUF_LEN as i32;
        self.mode = Mode::Writing;
    }

    // pub fn read_slice(&mut self, dst: &mut [u8]) -> usize {
    //     let len = cmp::min(dst.len(), self.remaining_u32() as usize);
    //     let cnt = len as i32;

    //     unsafe {
    //         ptr::copy_nonoverlapping(
    //             self.mem.as_ptr().offset(self.pos as isize),
    //             dst.as_mut_ptr(),
    //             len);
    //     }

    //     self.pos += cnt;
    //     len
    // }


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

    #[inline]
    fn pos(&self) -> usize {
        self.pos as usize
    }

    #[inline]
    fn lim(&self) -> usize {
        self.lim as usize
    }

    #[inline]
    fn remaining_u32(&self) -> i32 {
        self.lim - self.pos
    }

    #[inline]
    fn advance_impl(&mut self, mut cnt: usize) {
        cnt = cmp::min(cnt, self.remaining_u32() as usize);
        self.pos += cnt as i32;
    }
}

impl Buf for ByteBuf {
    #[inline]
    fn remaining(&self) -> usize {
        self.remaining_u32() as usize
    }

    #[inline]
    fn bytes<'a>(&'a self) -> &'a [u8] {
        if self.mode == Mode::Reading {
            &self.mem[self.pos()..self.lim()]
        }
        else {
            &self.mem[..self.pos()]
        }
    }

    #[inline]
    fn advance(&mut self, cnt: usize) {
        self.advance_impl(cnt)
    }
}

impl MutBuf for ByteBuf {
    fn remaining(&self) -> usize {
        self.remaining_u32() as usize
    }

    fn advance(&mut self, cnt: usize) {
        self.advance_impl(cnt)
    }

    fn mut_bytes<'a>(&'a mut self) -> &'a mut [u8] {
        assert!(self.mode == Mode::Writing);
        let pos = self.pos();
        let lim = self.lim();
        &mut self.mem[pos..lim]
    }
}
