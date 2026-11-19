//! Driving coding agents: harness discovery, the ACP registry, sign-in, setup
//! terminals and the per-bot actor that runs turns over ACP.

pub mod acp;
pub mod auth;
pub mod backends;
pub mod bot;
pub mod codex_audit;
pub mod config;
pub mod engine_permissions;
pub mod environment;
pub mod harness;
pub mod launcher;
pub mod new_bot;
pub mod node;
pub mod own_memory;
pub mod registry;
pub mod term;
pub mod workspace;

pub mod subscription;
