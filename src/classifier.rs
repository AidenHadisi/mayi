//! Ask a model whether a tool line is safe.
//!
//! Chat providers get `Verdict`'s JSON Schema and must return that object.
//! Markdown fences or extra prose are an error (fail closed).
//!
//! Jev has no schema: `safe` is noul ≥ 0.85 and `confidence` is bucketed from
//! noul (≥0.85 high, ≥0.50 medium, else low).

use std::fmt;
use std::time::Duration;

use schemars::{JsonSchema, schema_for};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::Result;
use crate::config::Config;

/// How sure the backend is. Required; missing values are an error.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
#[schemars(inline)]
pub enum Confidence {
    High,
    Medium,
    Low,
}

impl From<f64> for Confidence {
    fn from(noul: f64) -> Self {
        if noul >= 0.85 {
            Self::High
        } else if noul >= 0.50 {
            Self::Medium
        } else {
            Self::Low
        }
    }
}

/// Token counts reported by the backend, when it reports them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Usage {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
}

/// The classifier's answer. Chat providers must return exactly this object.
#[derive(Debug, Clone, PartialEq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Verdict {
    pub safe: bool,
    pub confidence: Confidence,
}

/// Which backend classifies tool calls.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    #[default]
    Jev,
    OpenAI,
    Anthropic,
}

impl fmt::Display for Provider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Jev => "jev",
            Self::OpenAI => "openai",
            Self::Anthropic => "anthropic",
        })
    }
}

impl Provider {
    /// Base URL used when `api_url` is not set.
    pub fn default_api_url(self) -> &'static str {
        match self {
            Self::Jev => "https://api.typesafe.ai",
            Self::OpenAI => "https://api.openai.com",
            Self::Anthropic => "https://api.anthropic.com",
        }
    }

    /// Model used when `model` is not set.
    pub fn default_model(self) -> &'static str {
        match self {
            Self::Jev => "jev-latest",
            Self::OpenAI => "gpt-5.6-luna",
            Self::Anthropic => "claude-haiku-4-5",
        }
    }

    /// Ask this backend whether `line` may run.
    pub fn run(line: &str, cfg: &Config) -> Result<(Verdict, Usage)> {
        let key = cfg
            .api_key
            .as_deref()
            .filter(|key| !key.is_empty())
            .ok_or("no API key: set api_key in config.toml")?;
        match cfg.provider {
            Self::Jev => jev(line, cfg, key),
            Self::OpenAI => openai(line, cfg, key),
            Self::Anthropic => anthropic(line, cfg, key),
        }
    }
}

fn http(cfg: &Config) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_millis(cfg.timeout_ms)))
        .build()
        .into()
}

fn jev(line: &str, cfg: &Config, key: &str) -> Result<(Verdict, Usage)> {
    let response: Value = http(cfg)
        .post(format!("{}/v1/systemone", cfg.api_url))
        .header("Authorization", format!("Bearer {key}"))
        .send_json(json!({
            "model": cfg.model,
            "state": line,
            "questions": {
                "gate": {
                    "type": "noul",
                    "instructions": cfg.instructions,
                }
            }
        }))?
        .body_mut()
        .read_json()?;

    let noul = response["answers"]["gate"]["noul"]
        .as_f64()
        .ok_or("missing noul")?;
    let confidence = Confidence::from(noul);

    Ok((
        Verdict {
            safe: confidence == Confidence::High,
            confidence,
        },
        Usage {
            input_tokens: response["usage"]["input_tokens"].as_u64(),
            output_tokens: response["usage"]["output_tokens"].as_u64(),
        },
    ))
}

fn openai(line: &str, cfg: &Config, key: &str) -> Result<(Verdict, Usage)> {
    let response: Value = http(cfg)
        .post(format!("{}/v1/chat/completions", cfg.api_url))
        .header("Authorization", format!("Bearer {key}"))
        .send_json(json!({
            "model": cfg.model,
            "messages": [
                {"role": "system", "content": cfg.instructions},
                {"role": "user", "content": line},
            ],
            "response_format": {
                "type": "json_schema",
                "json_schema": {
                    "name": "verdict",
                    "strict": true,
                    "schema": schema_for!(Verdict),
                },
            },
        }))?
        .body_mut()
        .read_json()?;

    let verdict: Verdict = serde_json::from_str(
        response["choices"][0]["message"]["content"]
            .as_str()
            .ok_or("missing content")?
            .trim(),
    )?;
    Ok((
        verdict,
        Usage {
            input_tokens: response["usage"]["prompt_tokens"].as_u64(),
            output_tokens: response["usage"]["completion_tokens"].as_u64(),
        },
    ))
}

fn anthropic(line: &str, cfg: &Config, key: &str) -> Result<(Verdict, Usage)> {
    let response: Value = http(cfg)
        .post(format!("{}/v1/messages", cfg.api_url))
        .header("x-api-key", key)
        .header("anthropic-version", "2023-06-01")
        .send_json(json!({
            "model": cfg.model,
            "max_tokens": 64,
            "system": cfg.instructions,
            "messages": [{"role": "user", "content": line}],
            "output_config": {
                "format": {
                    "type": "json_schema",
                    "schema": schema_for!(Verdict),
                },
            },
        }))?
        .body_mut()
        .read_json()?;

    let verdict: Verdict = serde_json::from_str(
        response["content"][0]["text"]
            .as_str()
            .ok_or("missing text")?
            .trim(),
    )?;
    Ok((
        verdict,
        Usage {
            input_tokens: response["usage"]["input_tokens"].as_u64(),
            output_tokens: response["usage"]["output_tokens"].as_u64(),
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::{Confidence, Provider, Verdict};
    use crate::config::Config;
    use httpmock::{Method::POST, MockServer};
    use schemars::schema_for;
    use serde_json::json;

    fn jev_config(server: &MockServer) -> Config {
        Config {
            provider: Provider::Jev,
            api_key: Some("test-key".into()),
            api_url: server.base_url(),
            ..Config::default()
        }
    }

    #[test]
    fn jev_sends_line_model_key_and_noul_question() {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(POST)
                .path("/v1/systemone")
                .header("authorization", "Bearer test-key")
                .json_body_includes(
                    json!({
                        "model": "jev-latest",
                        "state": "Bash npm test",
                        "questions": { "gate": { "type": "noul" } }
                    })
                    .to_string(),
                );
            then.status(200)
                .json_body(json!({ "answers": { "gate": { "type": "noul", "noul": 1.0 } } }));
        });

        let (verdict, _) = Provider::run("Bash npm test", &jev_config(&server)).expect("run");

        assert!(verdict.safe);
        mock.assert();
    }

    #[test]
    fn jev_missing_noul_is_err() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(POST).path("/v1/systemone");
            then.status(200)
                .json_body(json!({ "answers": { "gate": { "type": "noul" } } }));
        });

        let err = Provider::run("Bash ls", &jev_config(&server)).expect_err("noul");

        assert!(err.to_string().contains("noul"), "{err}");
    }

    #[test]
    fn jev_choice_answer_is_err() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(POST).path("/v1/systemone");
            then.status(200).json_body(json!({
                "answers": { "gate": { "type": "choice", "choice": "safe", "confidence": 1.0 } }
            }));
        });

        Provider::run("Bash ls", &jev_config(&server)).expect_err("choice");
    }

    #[test]
    fn jev_http_500_is_err() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(POST).path("/v1/systemone");
            then.status(500);
        });

        Provider::run("Bash ls", &jev_config(&server)).expect_err("500");
    }

    #[test]
    fn jev_usage_comes_back_beside_the_verdict() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(POST).path("/v1/systemone");
            then.status(200).json_body(json!({
                "answers": { "gate": { "type": "noul", "noul": 1.0 } },
                "usage": { "input_tokens": 42, "output_tokens": 7 }
            }));
        });

        let (verdict, usage) = Provider::run("Bash ls", &jev_config(&server)).expect("run");

        assert!(verdict.safe);
        assert_eq!(usage.input_tokens, Some(42));
        assert_eq!(usage.output_tokens, Some(7));
    }

    #[test]
    fn chat_safe_high() {
        let v: Verdict =
            serde_json::from_str("{\"safe\":true,\"confidence\":\"high\"}").expect("json");
        assert!(v.safe);
        assert_eq!(v.confidence, Confidence::High);
    }

    #[test]
    fn chat_unsafe_low() {
        let v: Verdict =
            serde_json::from_str("{\"safe\":false,\"confidence\":\"low\"}").expect("json");
        assert!(!v.safe);
        assert_eq!(v.confidence, Confidence::Low);
    }

    #[test]
    fn chat_missing_confidence_is_err() {
        serde_json::from_str::<Verdict>("{\"safe\":true}").expect_err("confidence");
    }

    #[test]
    fn chat_fenced_json_is_err() {
        serde_json::from_str::<Verdict>("```json\n{\"safe\":true,\"confidence\":\"high\"}\n```")
            .expect_err("fence");
    }

    #[test]
    fn empty_api_key_is_err() {
        let cfg = Config::default();
        let err = Provider::run("Bash ls", &cfg).expect_err("key");
        assert!(err.to_string().contains("no API key"), "{err}");
    }

    #[test]
    fn noul_buckets() {
        assert_eq!(Confidence::from(0.85), Confidence::High);
        assert_eq!(Confidence::from(0.50), Confidence::Medium);
        assert_eq!(Confidence::from(0.49), Confidence::Low);
    }

    #[test]
    fn verdict_schema_matches_chat_shape() {
        let schema = schema_for!(Verdict).to_value();
        assert_eq!(schema["type"], "object");
        assert_eq!(schema["additionalProperties"], false);
        assert_eq!(schema["required"], json!(["safe", "confidence"]));
        assert_eq!(
            schema["properties"]["confidence"]["enum"],
            json!(["high", "medium", "low"])
        );
    }
}
