//! Core domain logic for loq - a file size enforcement tool.
//!
//! This crate provides the foundational types and logic for enforcing file size
//! limits across a codebase. It handles configuration parsing, rule matching,
//! and violation reporting.
//!
//! # Architecture
//!
//! - [`config`]: Configuration types and compilation (glob patterns → matchers)
//! - [`parse`]: TOML parsing with unknown key detection and suggestions
//! - [`decide`]: Rule matching logic (exclude → rules → default)
//! - [`report`]: Outcome aggregation and finding generation
//! - [`format`](mod@format): Human-readable output formatting

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod config;
pub mod decide;
pub mod format;
pub mod parse;
pub mod report;

pub use config::{
    CompiledConfig, ConfigError, ConfigOrigin, LoqConfig, PatternList, Rule, Severity,
};
pub use decide::{Decision, MatchBy};
pub use parse::parse_config;
pub use report::{FileOutcome, Finding, FindingKind, OutcomeKind, Report, SkipReason, Summary};
