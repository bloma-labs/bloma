//! Instruction handlers.
//!
//! Account creation is split across several small instructions rather than
//! bundled: a context that carries more than one or two `init` constraints
//! generates enough stack frame to blow the 4096-byte limit. A front end
//! bundles the split instructions into one transaction, so the user experience
//! is unchanged.

pub mod admin;
pub mod forager;
pub mod risk;
pub mod settlement;
pub mod vault;

pub use admin::*;
pub use forager::*;
pub use risk::*;
pub use settlement::*;
pub use vault::*;
