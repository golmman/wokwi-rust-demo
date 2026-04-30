//! Pure-logic library for the wokwi-rust-demo clock firmware.
//!
//! Contains the modules that don't depend on `rp-pico` HAL, RTIC, or any
//! hardware peripherals — so they can be compiled and `cargo test`-ed against
//! the host target. The firmware binary (`src/main.rs`) consumes this
//! library; the binary additionally pulls in `bsp.rs` and the RTIC `mod app`
//! which **cannot** be host-tested.
//!
//! Run host tests with:
//! ```sh
//! cargo test --lib --target "$(rustc -vV | sed -n 's/host: //p')"
//! ```

#![cfg_attr(not(test), no_std)]

pub mod clock;
pub mod config;
pub mod display;
pub mod font;
