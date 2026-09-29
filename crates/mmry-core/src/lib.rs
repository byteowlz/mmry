//! Core library for mmry: the append-only workspace memory ledger, repository
//! discovery, configuration, and XDG path resolution.

pub mod agent_ctx;
pub mod cleanup;
pub mod config;
pub mod error;
pub mod memory_file;
pub mod paths;
pub mod preview;
pub mod repos;
pub mod store;
pub mod sync;

pub use agent_ctx::AgentCtx;
pub use error::Error;
pub use error::Result;
pub use memory_file::MemoryEntry;
pub use memory_file::MemoryEvent;
pub use memory_file::MemoryEventType;
pub use memory_file::MemoryFile;
pub use memory_file::MemoryType;
pub use memory_file::ScoredMemory;
