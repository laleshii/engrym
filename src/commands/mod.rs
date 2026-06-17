//! Command implementations. Each maps to a CLI subcommand in `main.rs`.
//! `agents` is a shared helper (not a command), used by `init` and `install`.

pub mod agents;
pub mod author;
pub mod deinit;
pub mod index;
pub mod init;
pub mod install;
pub mod lint;
pub mod related;
pub mod reset;
pub mod search;
pub mod show;
pub mod topic;
pub mod uninstall;
