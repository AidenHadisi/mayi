//! stdin → classifier → stdout. The process exits 0.
//!
//! A panic or error denies the call. Some hosts treat a crashed hook as allow.

use std::io::{self, Read};
use std::panic::{self, AssertUnwindSafe};
use std::process::ExitCode;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use serde_json::Value;

use crate::classifier::{Provider, Usage};
use crate::config::Config;
use crate::dialog::{self, Answer};
use crate::host::{Agent, Permission};
use crate::log::Event;

impl From<Answer> for Permission {
    fn from(answer: Answer) -> Self {
        match answer {
            Answer::Approve | Answer::Save => Self::Allow,
            Answer::Deny => Self::Deny,
        }
    }
}

/// Gate one tool call read from stdin and print the host's decision.
pub fn run(agent: Agent) -> ExitCode {
    let mut raw = String::new();
    let _ = io::stdin().read_to_string(&mut raw);
    let input: Value = serde_json::from_str(&raw).unwrap_or(Value::Null);

    let call = agent.parse(&raw);
    let config = Config::load();
    let started = Instant::now();
    let classified = config.and_then(|config| {
        panic::catch_unwind(|| Provider::run(&call.line, &config))
            .unwrap_or_else(|_| Err("panic".into()))
    });
    let latency_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);

    let (permission, usage, error, reason) = match classified {
        Ok((verdict, usage)) => {
            let permission = if verdict.safe {
                Permission::Allow
            } else {
                panic::catch_unwind(AssertUnwindSafe(|| dialog::ask(&call.line)))
                    .unwrap_or(Answer::Deny)
                    .into()
            };
            (permission, usage, false, format!("mayi: {permission}"))
        }
        Err(err) => (
            Permission::Deny,
            Usage::default(),
            true,
            format!("mayi: {err}"),
        ),
    };

    Event {
        ts: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        agent: agent.to_string(),
        tool: call.tool,
        verdict: Some(permission.to_string()),
        input_tokens: usage.input_tokens,
        output_tokens: usage.output_tokens,
        latency_ms: Some(latency_ms),
        error,
    }
    .append();

    println!("{}", agent.respond(&input, permission, &reason));
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::{Answer, Permission};

    #[test]
    fn approve_allows() {
        assert_eq!(Permission::from(Answer::Approve), Permission::Allow);
    }

    #[test]
    fn save_allows() {
        assert_eq!(Permission::from(Answer::Save), Permission::Allow);
    }

    #[test]
    fn deny_denies() {
        assert_eq!(Permission::from(Answer::Deny), Permission::Deny);
    }
}
