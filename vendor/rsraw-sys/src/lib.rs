#![allow(
    non_camel_case_types,
    non_upper_case_globals,
    clippy::approx_constant,
    clippy::redundant_static_lifetimes,
    non_snake_case
)]
#![no_std]

mod c_api {
    include!(concat!(env!("OUT_DIR"), "/bindings.rs"));
}

pub use c_api::*;
