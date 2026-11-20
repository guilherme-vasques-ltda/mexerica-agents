// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
//! Memory for coding-agent bots.
//!
//! The crate decides; the caller acts. It starts no processes, opens no network connections, calls no models
//! and spawns no threads. It reads and writes its own tables on the caller's SQLite connection, inside the
//! caller's transaction, so a memory change and whatever the caller records with it commit together.

pub mod check;
pub mod compaction;
pub mod dream;
pub mod evidence;
pub mod files;
pub mod lifecycle;
pub mod lineage;
pub mod nature;
pub mod opening;
pub mod origin;
pub mod profile;
mod rank;
pub mod redact;
pub mod schema;
pub mod store;
pub mod text;
pub mod tokens;

pub use rusqlite;
