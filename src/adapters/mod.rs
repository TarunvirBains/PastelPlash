//! Adapters for game-specific containers. The core works on PNG folders and never sees these
//! formats; an adapter decodes its container into images, hands each to the shared
//! [`crate::driver::Driver`], and writes the results back.

pub mod o2r;
