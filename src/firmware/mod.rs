//! Firmware releases: which one is newest (`version`), how they are stored (`store`),
//! the local file cache (`cache`) and the GitHub mirror that fills both (`sync`).

pub mod cache;
pub mod store;
pub mod sync;
pub mod version;

pub use store::{AssetRow, ReleaseRow};
pub use version::parse_version;
