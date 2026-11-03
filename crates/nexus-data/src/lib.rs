//! Static Elite Dangerous game knowledge, compiled into the binary.
//!
//! Everything here is pure data + pure functions (no I/O), so every other crate can depend on it
//! freely and it is trivially unit-testable.

pub mod bio_predict;
pub mod commodities;
pub mod engineering;
pub mod exobio;
pub mod exploration;
pub mod materials;
pub mod outfitting;
pub mod names;
pub mod ranks;
pub mod ships;
pub mod sku;
pub mod trader;
