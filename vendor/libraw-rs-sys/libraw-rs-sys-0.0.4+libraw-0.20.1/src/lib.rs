#![doc(html_root_url = "https://docs.rs/libraw-rs-sys/0.0.4")]
#![allow(
    non_camel_case_types,
    non_upper_case_globals,
    clippy::approx_constant,
    clippy::redundant_static_lifetimes,
    non_snake_case
)]
#![no_std]

#[cfg(feature = "bindgen")]
include!(concat!(env!("OUT_DIR"), "/bindings.rs"));
#[cfg(not(feature = "bindgen"))]
mod bindings;
#[cfg(not(feature = "bindgen"))]
pub use bindings::*;
