//! Permission gate for AI coding agents.
//!
//! The `mayi` binary is the hook and CLI. `bench` (feature `bench`) scores
//! the default classifiers and is not installed.

pub mod classifier;
pub mod config;
pub mod host;

#[doc(hidden)]
pub mod dialog;
#[doc(hidden)]
pub mod hook;
#[doc(hidden)]
pub mod log;

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
