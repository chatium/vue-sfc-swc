// sets the process-wide `#[global_allocator]` (mimalloc) for whatever binary
// links this crate
extern crate swc_malloc;

pub mod core;
pub mod dom;
pub mod sfc;
pub mod ssr;
pub mod ugc;
