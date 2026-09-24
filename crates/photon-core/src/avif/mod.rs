//! AVIF, decoded in pure Rust: `zenavif-parse` for the container, `rav1d` for the AV1
//! inside it. `image` decodes AVIF only through dav1d, a C library, and photon takes no
//! native dependencies (`docs/superpowers/specs/2026-09-24-photon-avif-design.md`).

#![allow(dead_code)] // Until decode_avif (Task 3) uses the wrapper.

mod av1;
