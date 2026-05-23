//! `Builders` trait — the convenience construction layer.
//!
//! Plain Rust trait declared here, implemented blanket over
//! [`crate::intern::TypeKernelDb`]. Production `ide-db` provides the
//! concrete db; sandbox uses [`crate::testing::InMemoryDb`].
//!
//! Filled in by Phase 1.E.
