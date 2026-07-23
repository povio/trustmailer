use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Priority {
    High,
    Normal,
    Low,
}

impl Priority {
    /// Value for the `X-Priority` header.
    pub fn x_priority(&self) -> &'static str {
        match self {
            Priority::High => "1 (Highest)",
            Priority::Normal => "3 (Normal)",
            Priority::Low => "5 (Lowest)",
        }
    }

    /// Value for the `Importance` header.
    pub fn importance(&self) -> &'static str {
        match self {
            Priority::High => "high",
            Priority::Normal => "normal",
            Priority::Low => "low",
        }
    }
}

/// An ordered list of extra header name/value pairs, applied verbatim (values are
/// still RFC 2047 encoded if non-ASCII when serialized).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HeaderList(pub Vec<(String, String)>);

impl HeaderList {
    pub fn push(&mut self, name: impl Into<String>, value: impl Into<String>) {
        self.0.push((name.into(), value.into()));
    }
}
