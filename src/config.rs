//! User policy at `$XDG_CONFIG_HOME/mayi/config.toml` (default
//! `~/.config/mayi/config.toml`). Every field is optional in the file;
//! missing keys keep the compiled defaults.
//!
//! Defaults for `api_url` and `model` follow `provider`. Set `api_url` only to
//! point at a proxy. `set` / `unset` edit one key in the sparse file. They do
//! not dump defaults.

use std::env;
use std::fmt;
use std::fs;
use std::io;
use std::path::PathBuf;

use clap::ValueEnum;
use serde::Deserialize;

use crate::Result;
use crate::classifier::Provider;

fn default_timeout_ms() -> u64 {
    2500
}

fn default_instructions() -> String {
    "Is this coding-agent tool call ordinary development work? \
        Prefer yes unless it is clearly harmful."
        .into()
}

/// Effective classifier settings: the file's values with defaults filled in.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default)]
    pub provider: Provider,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub api_url: String,
    #[serde(default)]
    pub model: String,
    #[serde(default = "default_timeout_ms")]
    pub timeout_ms: u64,
    #[serde(default = "default_instructions")]
    pub instructions: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            provider: Provider::Jev,
            api_key: None,
            api_url: String::new(),
            model: String::new(),
            timeout_ms: default_timeout_ms(),
            instructions: default_instructions(),
        }
        .resolve()
    }
}

/// A config key as named in the file and on the `config` CLI.
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
#[value(rename_all = "snake_case")]
pub enum Field {
    Provider,
    ApiKey,
    ApiUrl,
    Model,
    TimeoutMs,
    Instructions,
}

impl fmt::Display for Field {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.to_possible_value()
            .expect("Field has no skipped variants")
            .get_name()
            .fmt(f)
    }
}

impl Field {
    fn value(self, raw: &str) -> Result<toml::Value> {
        match self {
            Self::Provider => match raw {
                "jev" | "openai" | "anthropic" => Ok(toml::Value::String(raw.to_string())),
                _ => Err(format!("{self} must be jev, openai, or anthropic").into()),
            },
            Self::TimeoutMs => {
                let n: u64 = raw
                    .parse()
                    .map_err(|_| format!("{self} must be an integer"))?;
                let n = i64::try_from(n).map_err(|_| format!("{self} is too large"))?;
                Ok(toml::Value::Integer(n))
            }
            _ => Ok(toml::Value::String(raw.to_string())),
        }
    }
}

impl Config {
    /// `$XDG_CONFIG_HOME/mayi/config.toml`, else `~/.config/mayi/config.toml`.
    pub fn path() -> Option<PathBuf> {
        let base = match env::var_os("XDG_CONFIG_HOME") {
            Some(dir) if !dir.is_empty() => PathBuf::from(dir),
            _ => env::home_dir()?.join(".config"),
        };
        Some(base.join("mayi/config.toml"))
    }

    /// Defaults when the file is absent; an error when it exists but cannot be used.
    pub fn load() -> Result<Self> {
        let Some(path) = Self::path() else {
            return Ok(Self::default());
        };
        match fs::read_to_string(&path) {
            Ok(text) => toml::from_str(&text)
                .map(Self::resolve)
                .map_err(|e| format!("invalid config {}: {e}", path.display()).into()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(format!("cannot read config {}: {e}", path.display()).into()),
        }
    }

    fn resolve(mut self) -> Self {
        if self.api_url.is_empty() {
            self.api_url = self.provider.default_api_url().into();
        }
        if self.model.is_empty() {
            self.model = self.provider.default_model().into();
        }
        self
    }

    /// Effective value with compiled defaults filled in.
    pub fn get(&self, key: Field) -> String {
        match key {
            Field::Provider => self.provider.to_string(),
            Field::ApiKey => self.api_key.clone().unwrap_or_default(),
            Field::ApiUrl => self.api_url.clone(),
            Field::Model => self.model.clone(),
            Field::TimeoutMs => self.timeout_ms.to_string(),
            Field::Instructions => self.instructions.clone(),
        }
    }

    /// Print effective values as JSON. `api_key` is redacted unless `show_secrets`.
    pub fn print(show_secrets: bool) -> Result<()> {
        let loaded = Self::load()?;
        let mut api_key = loaded.api_key.clone().unwrap_or_default();
        if !show_secrets && !api_key.is_empty() {
            api_key = "****".into();
        }
        let body = serde_json::json!({
            "provider": loaded.provider.to_string(),
            "api_key": api_key,
            "api_url": loaded.api_url,
            "model": loaded.model,
            "timeout_ms": loaded.timeout_ms,
            "instructions": loaded.instructions,
        });
        println!("{}", serde_json::to_string_pretty(&body)?);
        Ok(())
    }

    /// Create or replace one key. Other keys in the file are left alone.
    pub fn set(key: Field, value: &str) -> Result<()> {
        let value = key.value(value)?;
        let path = Self::patch(|table| {
            table.insert(key.to_string(), value);
            Ok(())
        })?;
        eprintln!("set {key} → {}", path.display());
        Ok(())
    }

    /// Remove one key so the compiled default returns.
    pub fn unset(key: Field) -> Result<()> {
        let path = Self::patch(|table| {
            table.remove(&key.to_string());
            Ok(())
        })?;
        eprintln!("unset {key} → {}", path.display());
        Ok(())
    }

    /// Print the config file path.
    pub fn print_path() -> Result<()> {
        let path = Self::path().ok_or("cannot determine config path")?;
        println!("{}", path.display());
        Ok(())
    }

    /// Print one effective value.
    pub fn print_get(key: Field) -> Result<()> {
        println!("{}", Self::load()?.get(key));
        Ok(())
    }

    fn patch(edit: impl FnOnce(&mut toml::Table) -> Result<()>) -> Result<PathBuf> {
        let path = Self::path().ok_or("cannot determine config path")?;
        let mut table = match fs::read_to_string(&path) {
            Ok(text) if text.trim().is_empty() => toml::Table::new(),
            Ok(text) => text
                .parse()
                .map_err(|e| format!("invalid config {}: {e}", path.display()))?,
            Err(e) if e.kind() == io::ErrorKind::NotFound => toml::Table::new(),
            Err(e) => return Err(format!("cannot read config {}: {e}", path.display()).into()),
        };
        edit(&mut table)?;
        let mut body = toml::to_string_pretty(&table)?;
        if !body.ends_with('\n') {
            body.push('\n');
        }
        toml::from_str::<Config>(&body)
            .map_err(|e| format!("invalid config {}: {e}", path.display()))?;
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = fs::set_permissions(dir, fs::Permissions::from_mode(0o700));
            }
        }
        fs::write(&path, &body)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
        }
        Ok(path)
    }
}

#[cfg(test)]
mod tests {
    use clap::ValueEnum;

    use super::Field;

    #[test]
    fn known_keys_roundtrip() {
        for field in Field::value_variants() {
            let name = field.to_string();
            let parsed = Field::from_str(&name, false).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(parsed, *field);
            assert_eq!(parsed.to_string(), name);
        }
    }

    #[test]
    fn unknown_key_is_an_error() {
        let err = Field::from_str("not_a_key", false).expect_err("error");
        assert!(err.contains("invalid variant"), "{err}");
    }

    #[test]
    fn timeout_must_be_an_integer() {
        let err = Field::TimeoutMs.value("2.5").expect_err("error");
        assert!(err.to_string().contains("integer"), "{err}");
    }

    #[test]
    fn provider_must_be_known() {
        let err = Field::Provider.value("gemini").expect_err("error");
        assert!(err.to_string().contains("jev"), "{err}");
    }
}
