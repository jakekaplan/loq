#![forbid(unsafe_code)]

pub mod config;
pub mod decide;
pub mod format;
pub mod report;

pub use config::{CompiledConfig, ConfigError, ConfigOrigin, FenceConfig, Rule, Severity};
pub use decide::{Decision, MatchBy};
pub use report::{FileOutcome, Finding, FindingKind, OutcomeKind, Report, SkipReason, Summary};
