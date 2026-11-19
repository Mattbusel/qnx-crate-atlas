#![no_std]
//! Finds a byte without std. Builds for thumbv7em-none-eabihf.

pub fn find(hay: &[u8], b: u8) -> Option<usize> {
    memchr::memchr(b, hay)
}
