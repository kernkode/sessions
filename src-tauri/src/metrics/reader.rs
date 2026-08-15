//! Common interface for reading token usage.

use serde::Serialize;

/// Token usage of a session, as reported by the agent itself.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Usage {
    /// Last turn.
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    pub reasoning: u64,
    /// Session totals.
    pub total_input: u64,
    pub total_output: u64,
    /// Tokens currently occupying the context window.
    pub context_used: u64,
    /// Window reported by the agent.
    pub context_window: Option<u64>,
    pub model: Option<String>,
    pub turns: u32,
    /// The CLI's own session id, when it can be determined.
    pub external_id: Option<String>,
    /// Output tokens and duration of the last turn: the basis for tok/s.
    pub last_turn_output: u64,
    pub last_turn_ms: u64,
    /// Cost reported by the agent, if it reports one.
    pub cost_usd: Option<f64>,
}

impl Usage {
    /// tok/s of the last measured turn.
    pub fn turn_tps(&self) -> f64 {
        if self.last_turn_ms == 0 || self.last_turn_output == 0 {
            return 0.0;
        }
        self.last_turn_output as f64 * 1000.0 / self.last_turn_ms as f64
    }
}

/// Source of token usage for a specific agent.
pub trait UsageReader: Send {
    /// Returns the updated usage, or `None` when there is nothing new.
    fn poll(&mut self) -> Option<Usage>;
}

/// Null reader for agents without telemetry of their own (e.g. a plain terminal).
pub struct NullReader;

impl UsageReader for NullReader {
    fn poll(&mut self) -> Option<Usage> {
        None
    }
}
