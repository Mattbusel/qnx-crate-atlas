#![no_std]
//! A `no_std` byte finder for embedded systems. It does not build for a bare-metal target, because
//! the memchr dependency keeps its default `std` feature. `atlas claims` fails on it and prints the
//! `cargo tree` path that turned `std` on.

pub fn find(hay: &[u8], b: u8) -> Option<usize> {
    memchr::memchr(b, hay)
}
