//! Pure planning logic for the `edit_file`, `read_file`, and `write_file` tools.
//!
//! This crate intentionally performs no filesystem I/O and has no sandbox or session state.
//! Core owns execution so every read and write goes through the selected environment filesystem
//! with the appropriate sandbox context.

pub mod edit;
pub mod read;
pub mod spec;
pub mod write;
