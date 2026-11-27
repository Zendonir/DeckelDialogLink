//! DeckelLink core library (no GUI).
//!
//! Data transfer between a PC and a DECKEL FP2A with DIALOG 11. The library never
//! starts machine functions; unknown DIALOG 11 properties stay configurable and
//! are marked as unverified (see `docs/VERIFICATION_CHECKLIST.md`).
//!
//! Stability rules for this crate: no `unsafe`, no `unwrap`/`expect`/`panic`, no
//! unchecked indexing (enforced by workspace lints).
#![cfg_attr(
    test,
    allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing, clippy::float_cmp)
)]

pub mod appdata;
pub mod clock;
pub mod error;
pub mod model;
pub mod nc;
pub mod parameters;
pub mod profile;
pub mod scenarios;
pub mod storage;
pub mod transfer;
pub mod transport;
