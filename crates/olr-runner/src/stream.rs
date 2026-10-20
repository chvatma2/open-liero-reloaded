//! Lines of a session's `--output-format stream-json` output.

/// What one line means to the runner.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Line {
    /// The session's final line.
    Result { is_error: bool, text: String },
    /// Anything else: the session is alive.
    Activity,
}

pub fn scan(line: &str) -> Line {
    match serde_json::from_str::<serde_json::Value>(line) {
        Ok(v) if v["type"] == "result" => Line::Result {
            is_error: v["is_error"].as_bool().unwrap_or(false),
            text: v["result"].as_str().unwrap_or_default().to_string(),
        },
        _ => Line::Activity,
    }
}
