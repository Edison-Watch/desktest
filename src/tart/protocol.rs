//! Re-exports from the shared VM protocol module.
//!
//! The protocol implementation lives in `crate::vm_protocol`. This module
//! re-exports everything so that existing `use self::protocol::*` imports
//! in the Tart session code continue to work unchanged.

pub use crate::vm_protocol::*;
