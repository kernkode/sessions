//! Builds a session's command from its agent + provider.
//!
//! It assembles, in this order:
//!   1. `agent.args`
//!   2. resume flags (`resume_args` / `continue_args`)
//!   3. the provider's `args` for that agent (including Codex `-c` overrides)
//!   4. `agent.model_args`, only if the provider does not already set the model
//!   5. one-off arguments from the request
//!
//! And for the environment: `agent.env` first and `provider.env` after, so the
//! provider can override the agent's values.

use std::path::PathBuf;

use anyhow::{anyhow, Result};

use crate::config::agents::Agent;
use crate::config::providers::Provider;
use crate::config::ConfigStore;
use crate::model::CreateSessionRequest;

#[derive(Debug, Clone)]
pub struct LaunchPlan {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub cwd: PathBuf,
    /// Readable command line for the UI (no secrets).
    pub command_line: String,
    pub agent: Agent,
    pub provider: Option<Provider>,
}

pub fn plan(cfg: &ConfigStore, req: &CreateSessionRequest) -> Result<LaunchPlan> {
    let agent = cfg
        .agent(&req.agent_id)
        .ok_or_else(|| anyhow!("el agente «{}» no está definido en agents.toml", req.agent_id))?;

    let provider = req.provider_id.as_deref().and_then(|id| cfg.provider(id));
    if let (Some(pid), None) = (req.provider_id.as_deref(), provider.as_ref()) {
        return Err(anyhow!("el proveedor «{pid}» no está definido en providers.toml"));
    }
    if let Some(p) = &provider {
        if !p.supports_agent(&agent.id) {
            return Err(anyhow!(
                "el proveedor «{}» no está habilitado para el agente «{}»",
                p.id,
                agent.id
            ));
        }
    }

    let program = agent.resolve_program().ok_or_else(|| {
        anyhow!(
            "no se encontró el ejecutable «{}» del agente «{}» en el PATH",
            agent.platform_command(),
            agent.id
        )
    })?;

    let cwd = resolve_cwd(cfg, req)?;

    // Effective model: the requested one, or the provider's default.
    let model = req
        .model
        .clone()
        .or_else(|| provider.as_ref().and_then(|p| p.default_model.clone()));

    let mut args: Vec<String> = agent.args.clone();

    if let Some(ext) = &req.resume_external_id {
        for a in &agent.resume_args {
            args.push(a.replace("{session_id}", ext));
        }
    } else if req.continue_last {
        args.extend(agent.continue_args.iter().cloned());
    }

    let mut provider_sets_model = false;
    if let Some(p) = &provider {
        let extra = p.args_for(&agent.id, model.as_deref());
        provider_sets_model = p.injects_model(&agent.id);
        args.extend(extra);
    }

    if !provider_sets_model {
        if let Some(m) = &model {
            for a in &agent.model_args {
                args.push(a.replace("{model}", m));
            }
        }
    }

    args.extend(req.extra_args.iter().cloned());

    let mut env: Vec<(String, String)> =
        agent.env.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    if let Some(p) = &provider {
        let from_provider = p.env_for(&agent.id, model.as_deref());
        // The provider wins: duplicate keys from the agent are removed.
        env.retain(|(k, _)| !from_provider.iter().any(|(pk, _)| pk == k));
        env.extend(from_provider);
    }

    let command_line = format!(
        "{} {}",
        program.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default(),
        args.join(" ")
    )
    .trim()
    .to_string();

    Ok(LaunchPlan { program, args, env, cwd, command_line, agent, provider })
}

fn resolve_cwd(cfg: &ConfigStore, req: &CreateSessionRequest) -> Result<PathBuf> {
    let candidate = req
        .cwd
        .clone()
        .or_else(|| req.project_path.clone())
        .or_else(|| cfg.app_config().defaults.cwd.clone())
        .or_else(|| dirs::home_dir().map(|p| p.display().to_string()))
        .ok_or_else(|| anyhow!("no se pudo determinar el directorio de trabajo"))?;

    let p = PathBuf::from(&candidate);
    if !p.is_dir() {
        return Err(anyhow!("el directorio de trabajo «{candidate}» no existe"));
    }
    Ok(p)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::Paths;

    fn temp_cfg() -> (ConfigStore, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("sessions-launch-{}", uuid::Uuid::new_v4()));
        (ConfigStore::load(Paths::from_root(dir.clone())).unwrap(), dir)
    }

    /// Replaces agents.toml with one pointing at a real system executable.
    fn with_fake_agent(cfg: &ConfigStore) {
        let cmd = if cfg!(windows) { "cmd" } else { "sh" };
        let toml_src = format!(
            r#"
schema = 1

[[agent]]
id = "claude"
name = "Fake Claude"
command = "{cmd}"
args = ["--base"]
resume_args = ["--resume", "{{session_id}}"]
continue_args = ["--continue"]
model_args = ["--model", "{{model}}"]
metrics = "claude-jsonl"

[agent.env]
FORCE_COLOR = "1"
ANTHROPIC_BASE_URL = "agent-value"

[[agent]]
id = "codex"
command = "{cmd}"
model_args = ["-m", "{{model}}"]
metrics = "codex-rollout"
"#
        );
        std::fs::write(&cfg.paths().agents, toml_src).unwrap();
        cfg.reload();
    }

    fn request(agent: &str) -> CreateSessionRequest {
        CreateSessionRequest {
            agent_id: agent.into(),
            cwd: Some(std::env::temp_dir().display().to_string()),
            ..Default::default()
        }
    }

    #[test]
    fn basic_plan_with_the_anthropic_provider() {
        let (cfg, dir) = temp_cfg();
        with_fake_agent(&cfg);

        let mut req = request("claude");
        req.provider_id = Some("anthropic".into());
        req.model = Some("claude-sonnet-4-5".into());

        let p = plan(&cfg, &req).expect("plan");
        assert!(p.program.is_file());
        assert_eq!(p.args[0], "--base");

        let env: std::collections::BTreeMap<_, _> = p.env.iter().cloned().collect();
        // The provider overrides the value the agent had.
        assert_eq!(env["ANTHROPIC_BASE_URL"], "https://api.anthropic.com");
        assert_eq!(env["ANTHROPIC_MODEL"], "claude-sonnet-4-5");
        assert_eq!(env["FORCE_COLOR"], "1");
        // Anthropic passes the model via env, so it is not duplicated in args.
        assert!(!p.args.contains(&"--model".to_string()), "args: {:?}", p.args);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn codex_receives_the_provider_overrides() {
        let (cfg, dir) = temp_cfg();
        with_fake_agent(&cfg);

        let mut req = request("codex");
        req.provider_id = Some("openrouter".into());
        req.model = Some("deepseek/deepseek-v3.2".into());

        let p = plan(&cfg, &req).expect("plan");
        let line = p.args.join(" ");
        assert!(line.contains("model_providers.openrouter.base_url=https://openrouter.ai/api/v1"), "{line}");
        assert!(line.contains("model_provider=openrouter"), "{line}");
        assert!(line.contains("model=deepseek/deepseek-v3.2"), "{line}");
        // The provider already sets the model: no `-m` is added.
        assert!(!p.args.contains(&"-m".to_string()), "args: {:?}", p.args);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn agent_model_args_when_the_provider_does_not_set_it() {
        let (cfg, dir) = temp_cfg();
        with_fake_agent(&cfg);
        // Minimal provider with no model templates.
        std::fs::write(
            &cfg.paths().providers,
            r#"
[[provider]]
id = "simple"
kind = "custom"
base_url = "http://local"
api_key = "k"
agents = ["claude"]

[[provider.model]]
id = "m1"
context_window = 1000

[provider.env.claude]
ANTHROPIC_BASE_URL = "{base_url}"
"#,
        )
        .unwrap();
        cfg.reload();

        let mut req = request("claude");
        req.provider_id = Some("simple".into());
        req.model = Some("m1".into());
        let p = plan(&cfg, &req).unwrap();
        assert_eq!(p.args, vec!["--base", "--model", "m1"]);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn resume_and_continue() {
        let (cfg, dir) = temp_cfg();
        with_fake_agent(&cfg);

        let mut req = request("claude");
        req.resume_external_id = Some("abc-123".into());
        let p = plan(&cfg, &req).unwrap();
        assert_eq!(p.args, vec!["--base", "--resume", "abc-123"]);

        let mut req2 = request("claude");
        req2.continue_last = true;
        let p2 = plan(&cfg, &req2).unwrap();
        assert_eq!(p2.args, vec!["--base", "--continue"]);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_minimal_provider_without_blocks_configures_the_agent() {
        let (cfg, dir) = temp_cfg();
        with_fake_agent(&cfg);
        // A provider written "the simple way": no env, no args.
        std::fs::write(
            &cfg.paths().providers,
            r#"
[[provider]]
id = "gw"
name = "Gateway"
kind = "anthropic"
base_url = "https://gw.local"
api_key = "sk-1"
default_model = "m1"

[[provider.model]]
id = "m1"
context_window = 90000
max_output_tokens = 8000
"#,
        )
        .unwrap();
        cfg.reload();

        let mut req = request("claude");
        req.provider_id = Some("gw".into());
        let p = plan(&cfg, &req).expect("plan");
        let env: std::collections::BTreeMap<_, _> = p.env.iter().cloned().collect();
        assert_eq!(env["ANTHROPIC_BASE_URL"], "https://gw.local");
        assert_eq!(env["ANTHROPIC_API_KEY"], "sk-1");
        assert_eq!(env["ANTHROPIC_MODEL"], "m1");
        assert_eq!(env["CLAUDE_CODE_MAX_OUTPUT_TOKENS"], "8000");
        // The template already passes the model: not duplicated with --model.
        assert_eq!(p.args, vec!["--base"], "args: {:?}", p.args);

        // And that same provider is not offered to Codex.
        let mut req2 = request("codex");
        req2.provider_id = Some("gw".into());
        let err = plan(&cfg, &req2).unwrap_err().to_string();
        assert!(err.contains("no está habilitado para el agente"), "message: {err}");
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn clear_errors() {
        let (cfg, dir) = temp_cfg();
        with_fake_agent(&cfg);

        let mut req = request("does-not-exist");
        assert!(plan(&cfg, &req).unwrap_err().to_string().contains("agents.toml"));

        req = request("claude");
        req.provider_id = Some("ghost".into());
        assert!(plan(&cfg, &req).unwrap_err().to_string().contains("providers.toml"));

        req = request("claude");
        req.cwd = Some("Z:/path/that/does/not/exist/12345".into());
        assert!(plan(&cfg, &req).unwrap_err().to_string().contains("no existe"));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_missing_executable_gives_a_useful_message() {
        let (cfg, dir) = temp_cfg();
        std::fs::write(
            &cfg.paths().agents,
            r#"
schema = 1
[[agent]]
id = "ghost"
command = "nonexistent-binary-xyz-987"
metrics = "none"
"#,
        )
        .unwrap();
        cfg.reload();
        let err = plan(&cfg, &request("ghost")).unwrap_err().to_string();
        assert!(err.contains("PATH"), "message: {err}");
        std::fs::remove_dir_all(dir).ok();
    }
}
