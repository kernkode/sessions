//! Provider definitions (`~/.sessions/providers.toml`).
//!
//! A provider describes *which* endpoint/credentials to use and *how* to
//! translate that into the environment each CLI agent expects.
//!
//! That translation almost never needs to be written by hand: based on `kind` a
//! **default template** is applied (see [`env_template`] and [`args_template`])
//! with the variables and arguments each CLI expects. `[provider.env.<agent>]`
//! and `[provider.args]` are only for adding or fixing specific bits.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub const DEFAULT_PROVIDERS_TOML: &str = include_str!("../../assets/providers.default.toml");

/// Special key in `[provider.env.*]` that applies to every agent.
pub const ENV_ALL: &str = "all";

/// Agents that ship with built-in templates.
pub const KNOWN_AGENTS: [&str; 4] = ["claude", "codex", "opencode", "gemini"];

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProvidersFile {
    /// Schema version, for future migrations.
    #[serde(default = "default_schema")]
    pub schema: u32,
    #[serde(default, rename = "provider")]
    pub providers: Vec<Provider>,
}

fn default_schema() -> u32 {
    1
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProviderKind {
    Anthropic,
    OpenaiChat,
    OpenaiResponses,
    Google,
    Bedrock,
    Vertex,
    Ollama,
    #[default]
    Custom,
}

impl ProviderKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            ProviderKind::Anthropic => "anthropic",
            ProviderKind::OpenaiChat => "openai-chat",
            ProviderKind::OpenaiResponses => "openai-responses",
            ProviderKind::Google => "google",
            ProviderKind::Bedrock => "bedrock",
            ProviderKind::Vertex => "vertex",
            ProviderKind::Ollama => "ollama",
            ProviderKind::Custom => "custom",
        }
    }

    /// Every accepted value, to populate the UI dropdown.
    pub fn all() -> [ProviderKind; 8] {
        [
            ProviderKind::Anthropic,
            ProviderKind::OpenaiChat,
            ProviderKind::OpenaiResponses,
            ProviderKind::Google,
            ProviderKind::Bedrock,
            ProviderKind::Vertex,
            ProviderKind::Ollama,
            ProviderKind::Custom,
        ]
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Provider {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub kind: ProviderKind,
    #[serde(default = "yes")]
    pub enabled: bool,

    // ---- Connection ----
    #[serde(default)]
    pub base_url: Option<String>,
    /// Literal key. Prefer `api_key_env`, `api_key_file` or `api_key_command`.
    #[serde(default)]
    pub api_key: Option<String>,
    /// Name of a host environment variable holding the key.
    #[serde(default)]
    pub api_key_env: Option<String>,
    /// File containing the key. For JSON files, `api_key_json_path` points at the
    /// value inside the document (dot-separated keys).
    #[serde(default)]
    pub api_key_file: Option<String>,
    #[serde(default)]
    pub api_key_json_path: Option<String>,
    /// Command whose stdout is the key (secret managers).
    #[serde(default)]
    pub api_key_command: Option<String>,
    #[serde(default)]
    pub organization: Option<String>,
    #[serde(default)]
    pub project: Option<String>,
    #[serde(default)]
    pub region: Option<String>,
    #[serde(default)]
    pub api_version: Option<String>,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
    #[serde(default)]
    pub max_retries: Option<u32>,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,

    // ---- Models ----
    #[serde(default)]
    pub default_model: Option<String>,
    /// Cheap model for auxiliary tasks (titles, summaries...).
    #[serde(default)]
    pub small_model: Option<String>,
    #[serde(default, rename = "model")]
    pub models: Vec<Model>,

    // ---- Per-agent compatibility ----
    /// `env.all` + `env.<agent_id>`; values accept `{...}` templates.
    #[serde(default)]
    pub env: BTreeMap<String, BTreeMap<String, String>>,
    /// Extra CLI arguments per agent.
    #[serde(default)]
    pub args: BTreeMap<String, Vec<String>>,
    /// Allowed agents. Empty means "whatever the kind supports".
    #[serde(default)]
    pub agents: Vec<String>,

    #[serde(default)]
    pub notes: Option<String>,

    /// Computed when the configuration loads: agents this provider works with.
    /// Never written to the TOML nor accepted from the UI.
    #[serde(default, skip_deserializing)]
    pub supported_agents: Vec<String>,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Model {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    /// Context window in tokens. Drives the context indicator.
    #[serde(default)]
    pub context_window: Option<u64>,
    #[serde(default)]
    pub max_output_tokens: Option<u64>,
    #[serde(default)]
    pub reasoning: bool,
    #[serde(default)]
    pub vision: bool,
    #[serde(default = "yes")]
    pub tool_call: bool,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub pricing: Option<Pricing>,
    /// Real name to send to the endpoint when it differs from `id`.
    #[serde(default)]
    pub remote_id: Option<String>,
}

/// Prices in USD per million tokens.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct Pricing {
    #[serde(default)]
    pub input: f64,
    #[serde(default)]
    pub output: f64,
    #[serde(default)]
    pub cache_read: f64,
    #[serde(default)]
    pub cache_write: f64,
}

impl Pricing {
    /// Cost in USD for a specific token usage.
    pub fn cost(&self, input: u64, output: u64, cache_read: u64, cache_write: u64) -> f64 {
        const M: f64 = 1_000_000.0;
        (input as f64 * self.input
            + output as f64 * self.output
            + cache_read as f64 * self.cache_read
            + cache_write as f64 * self.cache_write)
            / M
    }
}

impl Provider {
    pub fn display_name(&self) -> &str {
        self.name.as_deref().unwrap_or(&self.id)
    }

    /// Computes the agents this provider can work with: the explicit `agents`
    /// list if present, otherwise those covered by its `kind` template plus any
    /// agent with its own `env`/`args` block.
    pub fn compute_supported_agents(&self) -> Vec<String> {
        if !self.agents.is_empty() {
            return self.agents.clone();
        }
        let mut out: Vec<String> = KNOWN_AGENTS
            .iter()
            .filter(|a| {
                !env_template(self.kind, a, &self.id).is_empty()
                    || !args_template(self.kind, a, &self.id).is_empty()
            })
            .map(|a| a.to_string())
            .collect();
        for key in self.env.keys().chain(self.args.keys()) {
            if key != ENV_ALL && !out.iter().any(|a| a == key) {
                out.push(key.clone());
            }
        }
        out
    }

    pub fn supports_agent(&self, agent_id: &str) -> bool {
        self.enabled && self.compute_supported_agents().iter().any(|a| a == agent_id)
    }

    pub fn find_model(&self, id: &str) -> Option<&Model> {
        self.models
            .iter()
            .find(|m| m.id == id || m.aliases.iter().any(|a| a == id))
    }

    /// Context window of the given model (or of the default model).
    pub fn context_window_for(&self, model: Option<&str>) -> Option<u64> {
        let key = model.or(self.default_model.as_deref())?;
        self.find_model(key)?.context_window
    }

    /// Where the key comes from, so the UI can explain it.
    pub fn key_source(&self) -> &'static str {
        if self.api_key.as_ref().is_some_and(|k| !k.is_empty()) {
            "literal"
        } else if self.api_key_file.as_ref().is_some_and(|k| !k.is_empty()) {
            "file"
        } else if self.api_key_command.as_ref().is_some_and(|k| !k.is_empty()) {
            "command"
        } else if self.api_key_env.as_ref().is_some_and(|k| !k.is_empty()) {
            "env"
        } else {
            "none"
        }
    }

    /// Resolves the API key. Precedence: literal > file > command > variable.
    pub fn resolve_api_key(&self) -> Option<String> {
        if let Some(k) = self.api_key.as_ref().filter(|k| !k.is_empty()) {
            return Some(k.clone());
        }
        if let Some(path) = self.api_key_file.as_ref().filter(|r| !r.is_empty()) {
            if let Some(v) = read_key_from_file(path, self.api_key_json_path.as_deref()) {
                if !v.is_empty() {
                    return Some(v);
                }
            }
        }
        if let Some(cmd) = self.api_key_command.as_ref().filter(|c| !c.is_empty()) {
            if let Some(out) = run_capture(cmd) {
                if !out.is_empty() {
                    return Some(out);
                }
            }
        }
        if let Some(var) = self.api_key_env.as_ref().filter(|v| !v.is_empty()) {
            if let Ok(v) = std::env::var(var) {
                if !v.is_empty() {
                    return Some(v);
                }
            }
        }
        None
    }

    /// Template variables available in `[provider.env.*]` and `args`.
    pub fn template_vars(&self, model: Option<&str>) -> BTreeMap<String, String> {
        let mut v = BTreeMap::new();
        let put = |v: &mut BTreeMap<String, String>, k: &str, val: Option<String>| {
            if let Some(val) = val {
                v.insert(k.to_string(), val);
            }
        };
        v.insert("provider_id".into(), self.id.clone());
        v.insert("provider_name".into(), self.display_name().to_string());
        v.insert("env_var".into(), env_var_for(&self.id));
        put(&mut v, "base_url", self.base_url.clone());
        put(&mut v, "api_key", self.resolve_api_key());
        put(&mut v, "organization", self.organization.clone());
        put(&mut v, "project", self.project.clone());
        put(&mut v, "region", self.region.clone());
        put(&mut v, "api_version", self.api_version.clone());
        put(&mut v, "small_model", self.small_model.clone());
        if let Some(t) = self.timeout_ms {
            v.insert("timeout_ms".into(), t.to_string());
        }
        if let Some(r) = self.max_retries {
            v.insert("max_retries".into(), r.to_string());
        }

        let model_id = model.map(str::to_string).or_else(|| self.default_model.clone());
        if let Some(id) = model_id {
            let remote = self
                .find_model(&id)
                .and_then(|m| m.remote_id.clone())
                .unwrap_or_else(|| id.clone());
            v.insert("model".into(), remote);
            v.insert("model_id".into(), id.clone());
            if let Some(m) = self.find_model(&id) {
                if let Some(cw) = m.context_window {
                    v.insert("context_window".into(), cw.to_string());
                }
                if let Some(mo) = m.max_output_tokens {
                    v.insert("max_output_tokens".into(), mo.to_string());
                }
            }
        }
        if let Some(home) = dirs::home_dir() {
            v.insert("home".into(), home.to_string_lossy().to_string());
        }
        v
    }

    /// Unresolved environment: `kind` template, then `env.all`, then
    /// `env.<agent_id>`. Later entries win.
    fn raw_env(&self, agent_id: &str) -> BTreeMap<String, String> {
        let mut raw = env_template(self.kind, agent_id, &self.id);
        for scope in [ENV_ALL, agent_id] {
            if let Some(block) = self.env.get(scope) {
                for (k, value) in block {
                    raw.insert(k.clone(), value.clone());
                }
            }
        }
        raw
    }

    /// Unresolved arguments: `args.all` plus the agent's own, or the `kind`
    /// template when there is no explicit block.
    fn raw_args(&self, agent_id: &str) -> Vec<String> {
        let own = match self.args.get(agent_id) {
            Some(l) => l.clone(),
            None => args_template(self.kind, agent_id, &self.id),
        };
        let mut out: Vec<String> = self.args.get(ENV_ALL).cloned().unwrap_or_default();
        out.extend(own);
        out
    }

    /// Environment to inject when launching `agent_id`.
    ///
    /// An empty value cancels a variable inherited from the template, and a
    /// template that cannot be resolved (e.g. a missing `api_key`) is dropped
    /// instead of leaking a literal `{api_key}` into the child process.
    pub fn env_for(&self, agent_id: &str, model: Option<&str>) -> Vec<(String, String)> {
        let vars = self.template_vars(model);
        let mut out: BTreeMap<String, String> = BTreeMap::new();
        for (k, raw) in self.raw_env(agent_id) {
            if raw.trim().is_empty() {
                continue;
            }
            if let Some(value) = render(&raw, &vars) {
                out.insert(k, value);
            }
        }
        out.into_iter().collect()
    }

    /// Extra arguments for `agent_id`.
    ///
    /// If the value of a pair like `-c key={model}` cannot be resolved, the flag
    /// preceding it is dropped too: leaving an orphan `-c` would make the CLI
    /// fail on startup.
    pub fn args_for(&self, agent_id: &str, model: Option<&str>) -> Vec<String> {
        let vars = self.template_vars(model);
        let mut out: Vec<String> = Vec::new();
        for raw in self.raw_args(agent_id) {
            match render(&raw, &vars) {
                Some(v) => out.push(v),
                None => {
                    if out.last().is_some_and(|prev| is_bare_flag(prev)) {
                        out.pop();
                    }
                }
            }
        }
        out
    }

    /// `true` if the provider already passes the model to the agent (via env or
    /// arguments): the agent's own `model_args` must then be skipped, or the
    /// model would be passed twice.
    pub fn injects_model(&self, agent_id: &str) -> bool {
        self.raw_env(agent_id).values().any(|v| v.contains("{model}"))
            || self.raw_args(agent_id).iter().any(|a| a.contains("{model}"))
    }
}

/// `-c`, `--config`...: a flag that expects a value next.
fn is_bare_flag(arg: &str) -> bool {
    arg.starts_with('-') && !arg.contains('=')
}

/// Environment variable name derived from the provider id.
pub fn env_var_for(provider_id: &str) -> String {
    let clean: String = provider_id
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_uppercase() } else { '_' })
        .collect();
    format!("{clean}_API_KEY")
}

/// Identifier usable as a TOML key (for Codex overrides).
fn toml_key(provider_id: &str) -> String {
    provider_id
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect()
}

/// Variables each CLI expects for a kind of endpoint. This is what removes the
/// need to write `[provider.env.*]` by hand in the common case.
pub fn env_template(
    kind: ProviderKind,
    agent_id: &str,
    provider_id: &str,
) -> BTreeMap<String, String> {
    use ProviderKind::*;
    let mut m = BTreeMap::new();
    let mut put = |k: &str, v: &str| {
        m.insert(k.to_string(), v.to_string());
    };
    match (kind, agent_id) {
        // Claude Code speaks the Anthropic protocol. `ANTHROPIC_API_KEY` is the
        // x-api-key header, which is what most compatible gateways expect; for
        // Bearer-style gateways override with ANTHROPIC_AUTH_TOKEN.
        (Anthropic, "claude") => {
            put("ANTHROPIC_BASE_URL", "{base_url}");
            put("ANTHROPIC_API_KEY", "{api_key}");
            put("ANTHROPIC_MODEL", "{model}");
            put("ANTHROPIC_SMALL_FAST_MODEL", "{small_model}");
            put("API_TIMEOUT_MS", "{timeout_ms}");
            put("CLAUDE_CODE_MAX_OUTPUT_TOKENS", "{max_output_tokens}");
        }
        (Anthropic, "opencode") => put("ANTHROPIC_API_KEY", "{api_key}"),
        // Codex reads the key from the variable declared by the `env_key` override.
        (OpenaiChat | OpenaiResponses | Ollama, "codex") => {
            put(&env_var_for(provider_id), "{api_key}");
        }
        (OpenaiResponses, "opencode") => put("OPENAI_API_KEY", "{api_key}"),
        (Google, "gemini") | (Google, "opencode") => {
            put("GEMINI_API_KEY", "{api_key}");
            put("GOOGLE_GENERATIVE_AI_API_KEY", "{api_key}");
        }
        _ => {}
    }
    m
}

/// Arguments each CLI needs in order to point at this endpoint.
pub fn args_template(kind: ProviderKind, agent_id: &str, provider_id: &str) -> Vec<String> {
    use ProviderKind::*;
    let id = toml_key(provider_id);
    match (kind, agent_id) {
        // Codex declares providers inline through `-c` overrides.
        (OpenaiChat | Ollama, "codex") => codex_overrides(&id, "chat", provider_id),
        (OpenaiResponses, "codex") => codex_overrides(&id, "responses", provider_id),
        (Google, "gemini") => vec!["--model".into(), "{model}".into()],
        _ => Vec::new(),
    }
}

fn codex_overrides(id: &str, wire_api: &str, provider_id: &str) -> Vec<String> {
    let var = env_var_for(provider_id);
    [
        "-c",
        &format!("model_providers.{id}.name={{provider_name}}"),
        "-c",
        &format!("model_providers.{id}.base_url={{base_url}}"),
        "-c",
        &format!("model_providers.{id}.env_key={var}"),
        "-c",
        &format!("model_providers.{id}.wire_api={wire_api}"),
        "-c",
        &format!("model_provider={id}"),
        "-c",
        "model={model}",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

/// Replaces `{key}` with its value. Returns `None` if any referenced key is
/// missing (which lets incomplete variables be dropped). `{{` escapes `{`.
pub fn render(template: &str, vars: &BTreeMap<String, String>) -> Option<String> {
    if !template.contains('{') {
        return Some(template.to_string());
    }
    let mut out = String::with_capacity(template.len() + 16);
    let bytes = template.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'{' if i + 1 < bytes.len() && bytes[i + 1] == b'{' => {
                out.push('{');
                i += 2;
            }
            b'}' if i + 1 < bytes.len() && bytes[i + 1] == b'}' => {
                out.push('}');
                i += 2;
            }
            b'{' => {
                let end = template[i + 1..].find('}')? + i + 1;
                let key = template[i + 1..end].trim();
                out.push_str(vars.get(key)?);
                i = end + 1;
            }
            _ => {
                let ch = template[i..].chars().next().unwrap();
                out.push(ch);
                i += ch.len_utf8();
            }
        }
    }
    Some(out)
}

/// Reads the key from a file: JSON with a key path, or plain text.
fn read_key_from_file(path: &str, json_path: Option<&str>) -> Option<String> {
    let contents = std::fs::read_to_string(expand_home(path)).ok()?;
    match json_path.filter(|p| !p.trim().is_empty()) {
        None => Some(contents.trim().to_string()),
        Some(p) => {
            let v: serde_json::Value = serde_json::from_str(&contents).ok()?;
            let mut current = &v;
            for segment in p.split('.').filter(|t| !t.is_empty()) {
                current = match current {
                    serde_json::Value::Array(a) => a.get(segment.parse::<usize>().ok()?)?,
                    other => other.get(segment)?,
                };
            }
            match current {
                serde_json::Value::String(s) => Some(s.trim().to_string()),
                other => Some(other.to_string()),
            }
        }
    }
}

/// Expands `~` to the user's home directory.
fn expand_home(path: &str) -> std::path::PathBuf {
    if let Some(rest) = path.strip_prefix("~/").or_else(|| path.strip_prefix("~\\")) {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    }
    std::path::PathBuf::from(path)
}

fn run_capture(cmd: &str) -> Option<String> {
    let parts = shell_words::split(cmd).ok()?;
    let (exe, args) = parts.split_first()?;
    let out = std::process::Command::new(exe).args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_providers_parse() {
        let f: ProvidersFile = toml::from_str(DEFAULT_PROVIDERS_TOML).expect("valid toml");
        assert!(f.providers.len() >= 3, "expected several providers");
        for p in &f.providers {
            assert!(!p.id.is_empty());
            assert!(!p.models.is_empty(), "{} has no models", p.id);
        }
        // Anthropic must configure Claude Code even without an explicit block:
        // the `kind` template takes care of it.
        let anth = f.providers.iter().find(|p| p.id == "anthropic").unwrap();
        let env: BTreeMap<_, _> = anth.env_for("claude", None).into_iter().collect();
        assert_eq!(env["ANTHROPIC_BASE_URL"], "https://api.anthropic.com");
        assert_eq!(env["ANTHROPIC_MODEL"], "claude-sonnet-4-5");
        assert!(anth.supports_agent("claude"));
    }

    #[test]
    fn render_substitutes_and_escapes() {
        let mut v = BTreeMap::new();
        v.insert("base_url".to_string(), "http://x".to_string());
        assert_eq!(render("{base_url}/v1", &v).unwrap(), "http://x/v1");
        assert_eq!(render("{{literal}}", &v).unwrap(), "{literal}");
        assert_eq!(render("no template", &v).unwrap(), "no template");
        assert!(render("{missing}", &v).is_none());
    }

    fn sample_provider() -> Provider {
        let toml_src = r#"
id = "acme"
kind = "openai-chat"
base_url = "https://api.acme.dev"
api_key = "sk-test"
default_model = "acme-large"

[[model]]
id = "acme-large"
context_window = 200000
remote_id = "acme/large-v2"

[env.all]
ACME_TOKEN = "{api_key}"

[env.claude]
ANTHROPIC_BASE_URL = "{base_url}/anthropic"
ANTHROPIC_API_KEY = "{api_key}"
ANTHROPIC_MODEL = "{model}"

[env.codex]
OPENAI_BASE_URL = "{base_url}/v1"

[args]
codex = ["-c", "model={model}"]
"#;
        toml::from_str(toml_src).unwrap()
    }

    #[test]
    fn per_agent_env_merges_all_and_specific() {
        let p = sample_provider();
        let map: BTreeMap<_, _> = p.env_for("claude", None).into_iter().collect();
        assert_eq!(map["ACME_TOKEN"], "sk-test");
        assert_eq!(map["ANTHROPIC_BASE_URL"], "https://api.acme.dev/anthropic");
        // `{model}` uses the default model's remote_id.
        assert_eq!(map["ANTHROPIC_MODEL"], "acme/large-v2");
        assert!(!map.contains_key("OPENAI_BASE_URL"));

        let codex: BTreeMap<_, _> = p.env_for("codex", None).into_iter().collect();
        assert_eq!(codex["OPENAI_BASE_URL"], "https://api.acme.dev/v1");
        assert_eq!(codex["ACME_TOKEN"], "sk-test");
    }

    #[test]
    fn unresolved_variables_are_dropped() {
        let mut p = sample_provider();
        p.api_key = None;
        p.api_key_env = Some("VAR_THAT_DOES_NOT_EXIST_12345".into());
        let map: BTreeMap<_, _> = p.env_for("claude", None).into_iter().collect();
        assert!(!map.contains_key("ANTHROPIC_API_KEY"));
        assert!(!map.contains_key("ACME_TOKEN"));
        assert!(map.contains_key("ANTHROPIC_BASE_URL"));
    }

    #[test]
    fn context_window_and_cost() {
        let p = sample_provider();
        assert_eq!(p.context_window_for(None), Some(200_000));
        let pr = Pricing { input: 3.0, output: 15.0, cache_read: 0.3, cache_write: 3.75 };
        assert!((pr.cost(1_000_000, 0, 0, 0) - 3.0).abs() < 1e-9);
    }

    // ---- Default templates per endpoint kind ----

    fn minimal(id: &str, kind: ProviderKind) -> Provider {
        Provider {
            id: id.into(),
            kind,
            enabled: true,
            base_url: Some("https://api.gw.dev/v1".into()),
            api_key: Some("sk-abc".into()),
            default_model: Some("m1".into()),
            models: vec![Model {
                id: "m1".into(),
                context_window: Some(1000),
                max_output_tokens: Some(500),
                ..Default::default()
            }],
            ..Default::default()
        }
    }

    #[test]
    fn anthropic_without_blocks_configures_claude_code() {
        let p = minimal("gw", ProviderKind::Anthropic);
        let env: BTreeMap<_, _> = p.env_for("claude", None).into_iter().collect();
        assert_eq!(env["ANTHROPIC_BASE_URL"], "https://api.gw.dev/v1");
        assert_eq!(env["ANTHROPIC_API_KEY"], "sk-abc");
        assert_eq!(env["ANTHROPIC_MODEL"], "m1");
        assert_eq!(env["CLAUDE_CODE_MAX_OUTPUT_TOKENS"], "500");
        // Without small_model or timeout those variables are not invented.
        assert!(!env.contains_key("ANTHROPIC_SMALL_FAST_MODEL"));
        assert!(!env.contains_key("API_TIMEOUT_MS"));
        assert!(p.args_for("claude", None).is_empty());
        assert!(p.supports_agent("claude"));
        assert!(!p.supports_agent("codex"), "an Anthropic endpoint is no use to Codex");
        assert!(!p.supports_agent("shell"));
    }

    #[test]
    fn openai_chat_without_blocks_configures_codex() {
        let p = minimal("seek-ai", ProviderKind::OpenaiChat);
        let env: BTreeMap<_, _> = p.env_for("codex", None).into_iter().collect();
        assert_eq!(env["SEEK_AI_API_KEY"], "sk-abc", "variable derived from the provider id");

        let line = p.args_for("codex", Some("m1")).join(" ");
        assert!(line.contains("model_providers.seek_ai.base_url=https://api.gw.dev/v1"), "{line}");
        assert!(line.contains("model_providers.seek_ai.env_key=SEEK_AI_API_KEY"), "{line}");
        assert!(line.contains("model_providers.seek_ai.wire_api=chat"), "{line}");
        assert!(line.contains("model_provider=seek_ai"), "{line}");
        assert!(line.contains("model=m1"), "{line}");
        assert!(p.supports_agent("codex"));
        assert!(!p.supports_agent("claude"));
    }

    #[test]
    fn openai_responses_uses_wire_api_responses() {
        let p = minimal("oai", ProviderKind::OpenaiResponses);
        assert!(p.args_for("codex", None).join(" ").contains("wire_api=responses"));
        // And OpenCode gets the standard variable.
        let env: BTreeMap<_, _> = p.env_for("opencode", None).into_iter().collect();
        assert_eq!(env["OPENAI_API_KEY"], "sk-abc");
    }

    #[test]
    fn own_blocks_extend_and_cancel_the_template() {
        let mut p = minimal("gw", ProviderKind::Anthropic);
        let mut claude = BTreeMap::new();
        // Adds a new variable...
        claude.insert("ANTHROPIC_CUSTOM_HEADERS".to_string(), "X-Foo: bar".to_string());
        // ...fixes another...
        claude.insert("ANTHROPIC_BASE_URL".to_string(), "{base_url}/anthropic".to_string());
        // ...and cancels an inherited one with an empty value.
        claude.insert("CLAUDE_CODE_MAX_OUTPUT_TOKENS".to_string(), String::new());
        p.env.insert("claude".into(), claude);

        let env: BTreeMap<_, _> = p.env_for("claude", None).into_iter().collect();
        assert_eq!(env["ANTHROPIC_BASE_URL"], "https://api.gw.dev/v1/anthropic");
        assert_eq!(env["ANTHROPIC_CUSTOM_HEADERS"], "X-Foo: bar");
        assert_eq!(env["ANTHROPIC_API_KEY"], "sk-abc", "the rest is inherited");
        assert!(!env.contains_key("CLAUDE_CODE_MAX_OUTPUT_TOKENS"), "cancelled with an empty value");
    }

    #[test]
    fn own_args_replace_the_template() {
        let mut p = minimal("gw", ProviderKind::OpenaiChat);
        p.args.insert("codex".into(), vec!["-m".into(), "{model}".into()]);
        assert_eq!(p.args_for("codex", None), vec!["-m", "m1"]);
        // `all` is always added.
        p.args.insert(ENV_ALL.into(), vec!["--quiet".into()]);
        assert_eq!(p.args_for("codex", None), vec!["--quiet", "-m", "m1"]);
    }

    #[test]
    fn no_orphan_flags_when_the_value_is_missing() {
        // Provider without models: `{model}` cannot be resolved.
        let mut p = minimal("gw", ProviderKind::OpenaiChat);
        p.models.clear();
        p.default_model = None;
        let args = p.args_for("codex", None);
        assert!(!args.iter().any(|a| a.contains("model=")), "args: {args:?}");
        assert_ne!(args.last().map(String::as_str), Some("-c"), "would leave a valueless -c");
        // Complete pairs survive.
        assert!(args.iter().any(|a| a.contains("model_provider=gw")), "args: {args:?}");
    }

    #[test]
    fn explicit_agents_win_over_inference() {
        let mut p = minimal("gw", ProviderKind::Anthropic);
        p.agents = vec!["claude".into(), "opencode".into()];
        assert_eq!(p.compute_supported_agents(), vec!["claude", "opencode"]);
        p.enabled = false;
        assert!(!p.supports_agent("claude"), "a disabled provider supports nothing");
    }

    // ---- Key sources ----

    #[test]
    fn key_from_json_file_and_from_text() {
        let dir = std::env::temp_dir().join(format!("sessions-key-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let json = dir.join("auth.json");
        std::fs::write(&json, br#"{"gorouter":{"type":"api_key","key":"sk-from-json"}}"#).unwrap();
        let txt = dir.join("key.txt");
        std::fs::write(&txt, b"  sk-from-text\n").unwrap();

        let mut p = minimal("gw", ProviderKind::Anthropic);
        p.api_key = None;
        p.api_key_file = Some(json.display().to_string());
        p.api_key_json_path = Some("gorouter.key".into());
        assert_eq!(p.resolve_api_key().as_deref(), Some("sk-from-json"));
        assert_eq!(p.key_source(), "file");

        p.api_key_json_path = None;
        p.api_key_file = Some(txt.display().to_string());
        assert_eq!(p.resolve_api_key().as_deref(), Some("sk-from-text"));

        // A path that does not exist inside the JSON does not blow up.
        p.api_key_file = Some(json.display().to_string());
        p.api_key_json_path = Some("no.such.path".into());
        assert!(p.resolve_api_key().is_none());

        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn key_source_precedence() {
        let mut p = minimal("gw", ProviderKind::Anthropic);
        p.api_key_env = Some("NONEXISTENT_VAR_X".into());
        // The literal wins.
        assert_eq!(p.resolve_api_key().as_deref(), Some("sk-abc"));
        assert_eq!(p.key_source(), "literal");
        p.api_key = None;
        assert_eq!(p.key_source(), "env");
        assert!(p.resolve_api_key().is_none(), "the variable does not exist");
    }

    #[test]
    fn variable_name_derived_from_the_id() {
        assert_eq!(env_var_for("zai-coding-cn"), "ZAI_CODING_CN_API_KEY");
        assert_eq!(env_var_for("gpt.5"), "GPT_5_API_KEY");
    }
}
