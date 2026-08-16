//! Builds a session's command from its agent.
//!
//! It assembles, in this order:
//!   1. `agent.args`
//!   2. resume flags (`resume_args` / `continue_args`)
//!   3. one-off arguments from the request
//!
//! Providers, models and credentials are managed by each CLI itself: this app
//! only launches the process and reads its telemetry.

use std::path::PathBuf;

use anyhow::{anyhow, Result};

use crate::config::agents::Agent;
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
}

pub fn plan(cfg: &ConfigStore, req: &CreateSessionRequest) -> Result<LaunchPlan> {
    let agent = cfg
        .agent(&req.agent_id)
        .ok_or_else(|| anyhow!("agent '{}' is not defined in agents.toml", req.agent_id))?;

    let program = agent.resolve_program().ok_or_else(|| {
        anyhow!(
            "executable '{}' for agent '{}' not found on PATH",
            agent.platform_command(),
            agent.id
        )
    })?;

    let cwd = resolve_cwd(cfg, req)?;

    let mut args: Vec<String> = agent.args.clone();

    if let Some(ext) = &req.resume_external_id {
        for a in &agent.resume_args {
            args.push(a.replace("{session_id}", ext));
        }
    } else if req.continue_last {
        args.extend(agent.continue_args.iter().cloned());
    }

    args.extend(req.extra_args.iter().cloned());

    let env: Vec<(String, String)> =
        agent.env.iter().map(|(k, v)| (k.clone(), v.clone())).collect();

    let command_line = format!(
        "{} {}",
        program.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default(),
        args.join(" ")
    )
    .trim()
    .to_string();

    Ok(LaunchPlan { program, args, env, cwd, command_line, agent })
}

fn resolve_cwd(cfg: &ConfigStore, req: &CreateSessionRequest) -> Result<PathBuf> {
    let candidate = req
        .cwd
        .clone()
        .or_else(|| req.project_path.clone())
        .or_else(|| cfg.app_config().defaults.cwd.clone())
        .or_else(|| dirs::home_dir().map(|p| p.display().to_string()))
        .ok_or_else(|| anyhow!("could not determine the working directory"))?;

    let p = PathBuf::from(&candidate);
    if !p.is_dir() {
        return Err(anyhow!("working directory '{candidate}' does not exist"));
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
metrics = "claude-jsonl"

[agent.env]
FORCE_COLOR = "1"
ANTHROPIC_BASE_URL = "agent-value"
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
    fn basic_plan_passes_the_agent_env() {
        let (cfg, dir) = temp_cfg();
        with_fake_agent(&cfg);

        let p = plan(&cfg, &request("claude")).expect("plan");
        assert!(p.program.is_file());
        assert_eq!(p.args, vec!["--base"]);
        let env: std::collections::BTreeMap<_, _> = p.env.iter().cloned().collect();
        assert_eq!(env["FORCE_COLOR"], "1");
        assert_eq!(env["ANTHROPIC_BASE_URL"], "agent-value");
        // The readable line is «executable + args»; on Windows the name keeps its
        // extension (cmd.exe).
        let cmd = if cfg!(windows) { "cmd" } else { "sh" };
        assert!(p.command_line.starts_with(cmd), "{}", p.command_line);
        assert!(p.command_line.ends_with(" --base"), "{}", p.command_line);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn extra_args_from_the_request_are_appended() {
        let (cfg, dir) = temp_cfg();
        with_fake_agent(&cfg);

        let mut req = request("claude");
        req.extra_args = vec!["--model".into(), "custom".into()];
        let p = plan(&cfg, &req).unwrap();
        assert_eq!(p.args, vec!["--base", "--model", "custom"]);
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
    fn clear_errors() {
        let (cfg, dir) = temp_cfg();
        with_fake_agent(&cfg);

        let req = request("does-not-exist");
        assert!(plan(&cfg, &req).unwrap_err().to_string().contains("agents.toml"));

        let mut req = request("claude");
        req.cwd = Some("Z:/path/that/does/not/exist/12345".into());
        assert!(plan(&cfg, &req).unwrap_err().to_string().contains("does not exist"));
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
