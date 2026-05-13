//! File budget units and limits.

use serde::Serialize;

/// The unit used to measure a file budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Metric {
    /// Physical line count.
    Lines,
    /// Approximate token count.
    Tokens,
}

impl Metric {
    /// Returns the serialized name used in JSON output.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Lines => "lines",
            Self::Tokens => "tokens",
        }
    }

    /// Returns true when measurements for this metric are approximate.
    #[must_use]
    pub const fn is_approximate(self) -> bool {
        matches!(self, Self::Tokens)
    }
}

/// A maximum file budget in one metric.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Limit {
    /// Budget unit.
    pub metric: Metric,
    /// Maximum allowed value.
    pub max: usize,
}

impl Limit {
    /// Creates a line limit.
    #[must_use]
    pub const fn lines(max: usize) -> Self {
        Self {
            metric: Metric::Lines,
            max,
        }
    }

    /// Creates an approximate token limit.
    #[must_use]
    pub const fn tokens(max: usize) -> Self {
        Self {
            metric: Metric::Tokens,
            max,
        }
    }

    /// Returns true when measurements for this limit are approximate.
    #[must_use]
    pub const fn is_approximate(self) -> bool {
        self.metric.is_approximate()
    }
}
