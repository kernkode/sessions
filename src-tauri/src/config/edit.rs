//! Editing `providers.toml` while preserving comments and formatting.
//!
//! `toml_edit` is used instead of reserializing the file: the TOML files in
//! `~/.sessions` are commented and owned by the user, so an edit from the UI must
//! only touch the affected block.

use std::path::Path;

use anyhow::{Context, Result};
use toml_edit::{Array, ArrayOfTables, DocumentMut, Item, Table, Value};

use super::providers::{Model, Provider};

/// Inserts or replaces the provider with that `id`. Returns `true` if it was new.
pub fn upsert_provider(path: &Path, p: &Provider) -> Result<bool> {
    let mut doc = read(path)?;
    let arr = providers_array(&mut doc);

    let idx = (0..arr.len()).find(|i| id_of(arr.get(*i)) == Some(p.id.as_str()));
    let fresh = provider_table(p);
    match idx {
        Some(i) => {
            if let Some(t) = arr.get_mut(i) {
                // Keeps the comment preceding the block, if any.
                let decor = t.decor().clone();
                *t = fresh;
                *t.decor_mut() = decor;
            }
            write(path, &doc)?;
            Ok(false)
        }
        None => {
            arr.push(fresh);
            write(path, &doc)?;
            Ok(true)
        }
    }
}

/// Removes a provider. Returns `true` if it existed.
///
/// The comment preceding the block moves to the next provider (or to the end of
/// the document): in a commented file those comments belong to the user and must
/// not disappear with the block.
pub fn remove_provider(path: &Path, id: &str) -> Result<bool> {
    let mut doc = read(path)?;
    let arr = providers_array(&mut doc);
    let idx = (0..arr.len()).find(|i| id_of(arr.get(*i)) == Some(id));
    match idx {
        Some(i) => {
            let comment = arr.get(i).and_then(|t| comments_of(t.decor().prefix()));
            arr.remove(i);
            if let Some(c) = comment {
                if let Some(next) = arr.get_mut(i) {
                    let previous = next
                        .decor()
                        .prefix()
                        .and_then(|r| r.as_str())
                        .unwrap_or("")
                        .to_string();
                    next.decor_mut().set_prefix(format!("{c}{previous}"));
                } else {
                    let tail = doc.trailing().as_str().unwrap_or("").to_string();
                    doc.set_trailing(format!("{tail}{c}"));
                }
            }
            write(path, &doc)?;
            Ok(true)
        }
        None => Ok(false),
    }
}

fn read(path: &Path) -> Result<DocumentMut> {
    let src = std::fs::read_to_string(path).unwrap_or_default();
    src.parse::<DocumentMut>().with_context(|| {
        format!("{} no es TOML válido; corrígelo antes de editarlo desde la app", path.display())
    })
}

fn write(path: &Path, doc: &DocumentMut) -> Result<()> {
    crate::paths::write_atomic(path, doc.to_string().as_bytes())
}

fn providers_array(doc: &mut DocumentMut) -> &mut ArrayOfTables {
    if !doc.contains_key("provider") || doc["provider"].as_array_of_tables().is_none() {
        doc["provider"] = Item::ArrayOfTables(ArrayOfTables::new());
    }
    doc["provider"].as_array_of_tables_mut().expect("providers array")
}

fn id_of(t: Option<&Table>) -> Option<&str> {
    t?.get("id")?.as_str()
}

fn put_str(t: &mut Table, key: &str, value: &Option<String>) {
    if let Some(v) = value.as_ref().filter(|s| !s.trim().is_empty()) {
        t.insert(key, item(v.as_str()));
    }
}

fn item<V: Into<Value>>(v: V) -> Item {
    Item::Value(v.into())
}

fn provider_table(p: &Provider) -> Table {
    let mut t = Table::new();
    t.insert("id", item(p.id.as_str()));
    put_str(&mut t, "name", &p.name);
    t.insert("kind", item(p.kind.as_str()));
    if !p.enabled {
        t.insert("enabled", item(false));
    }
    put_str(&mut t, "base_url", &p.base_url);
    put_str(&mut t, "api_key", &p.api_key);
    put_str(&mut t, "api_key_env", &p.api_key_env);
    put_str(&mut t, "api_key_file", &p.api_key_file);
    put_str(&mut t, "api_key_json_path", &p.api_key_json_path);
    put_str(&mut t, "api_key_command", &p.api_key_command);
    put_str(&mut t, "organization", &p.organization);
    put_str(&mut t, "project", &p.project);
    put_str(&mut t, "region", &p.region);
    put_str(&mut t, "api_version", &p.api_version);
    if let Some(v) = p.timeout_ms {
        t.insert("timeout_ms", item(v as i64));
    }
    if let Some(v) = p.max_retries {
        t.insert("max_retries", item(v as i64));
    }
    put_str(&mut t, "default_model", &p.default_model);
    put_str(&mut t, "small_model", &p.small_model);
    if !p.agents.is_empty() {
        t.insert("agents", Item::Value(Value::Array(str_array(&p.agents))));
    }
    put_str(&mut t, "notes", &p.notes);

    if !p.headers.is_empty() {
        let mut h = Table::new();
        for (k, v) in &p.headers {
            h.insert(k, item(v.as_str()));
        }
        t.insert("headers", Item::Table(h));
    }

    if !p.models.is_empty() {
        let mut models = ArrayOfTables::new();
        for m in &p.models {
            models.push(model_table(m));
        }
        t.insert("model", Item::ArrayOfTables(models));
    }

    if !p.env.is_empty() {
        let mut env = Table::new();
        env.set_implicit(true);
        for (agent, vars) in &p.env {
            if vars.is_empty() {
                continue;
            }
            let mut sub = Table::new();
            for (k, v) in vars {
                sub.insert(k, item(v.as_str()));
            }
            env.insert(agent, Item::Table(sub));
        }
        if !env.is_empty() {
            t.insert("env", Item::Table(env));
        }
    }

    if !p.args.is_empty() {
        let mut args = Table::new();
        let mut any = false;
        for (agent, list) in &p.args {
            if list.is_empty() {
                continue;
            }
            args.insert(agent, Item::Value(Value::Array(str_array(list))));
            any = true;
        }
        if any {
            t.insert("args", Item::Table(args));
        }
    }

    t
}

fn model_table(m: &Model) -> Table {
    let mut t = Table::new();
    t.insert("id", item(m.id.as_str()));
    put_str(&mut t, "name", &m.name);
    put_str(&mut t, "remote_id", &m.remote_id);
    if let Some(v) = m.context_window {
        t.insert("context_window", item(v as i64));
    }
    if let Some(v) = m.max_output_tokens {
        t.insert("max_output_tokens", item(v as i64));
    }
    if m.reasoning {
        t.insert("reasoning", item(true));
    }
    if m.vision {
        t.insert("vision", item(true));
    }
    if !m.tool_call {
        t.insert("tool_call", item(false));
    }
    if !m.aliases.is_empty() {
        t.insert("aliases", Item::Value(Value::Array(str_array(&m.aliases))));
    }
    if let Some(pr) = m.pricing {
        let any = pr.input > 0.0 || pr.output > 0.0 || pr.cache_read > 0.0 || pr.cache_write > 0.0;
        if any {
            let mut p = Table::new();
            p.insert("input", item(pr.input));
            p.insert("output", item(pr.output));
            if pr.cache_read > 0.0 {
                p.insert("cache_read", item(pr.cache_read));
            }
            if pr.cache_write > 0.0 {
                p.insert("cache_write", item(pr.cache_write));
            }
            t.insert("pricing", Item::Table(p));
        }
    }
    t
}

fn str_array(v: &[String]) -> Array {
    let mut a = Array::new();
    for s in v {
        a.push(s.as_str());
    }
    a
}

/// Extracts only the comment lines from a decoration prefix.
fn comments_of(prefix: Option<&toml_edit::RawString>) -> Option<String> {
    let text = prefix?.as_str()?;
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim_end)
        .filter(|l| l.trim_start().starts_with('#'))
        .collect();
    if lines.is_empty() {
        return None;
    }
    Some(format!("{}\n", lines.join("\n")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::providers::{Pricing, ProviderKind, ProvidersFile};
    use std::collections::BTreeMap;

    fn file(contents: &str) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("sessions-edit-{}.toml", uuid::Uuid::new_v4()));
        std::fs::write(&p, contents).unwrap();
        p
    }

    fn sample(id: &str) -> Provider {
        let mut env = BTreeMap::new();
        let mut claude = BTreeMap::new();
        claude.insert("ANTHROPIC_BASE_URL".to_string(), "{base_url}".to_string());
        claude.insert("ANTHROPIC_AUTH_TOKEN".to_string(), "{api_key}".to_string());
        env.insert("claude".to_string(), claude);

        let mut args = BTreeMap::new();
        args.insert("codex".to_string(), vec!["-c".to_string(), "model={model}".to_string()]);

        Provider {
            id: id.to_string(),
            name: Some("Sample".into()),
            kind: ProviderKind::OpenaiChat,
            enabled: true,
            base_url: Some("https://api.sample.dev/v1".into()),
            api_key_env: Some("SAMPLE_KEY".into()),
            default_model: Some("model-1".into()),
            timeout_ms: Some(120_000),
            models: vec![Model {
                id: "model-1".into(),
                name: Some("Model One".into()),
                context_window: Some(200_000),
                max_output_tokens: Some(32_000),
                reasoning: true,
                tool_call: true,
                pricing: Some(Pricing { input: 1.5, output: 7.5, cache_read: 0.15, cache_write: 0.0 }),
                ..Default::default()
            }],
            env,
            args,
            ..Default::default()
        }
    }

    #[test]
    fn adds_a_provider_preserving_comments() {
        let path = file("# important header\nschema = 1\n\n# previous provider\n[[provider]]\nid = \"old\"\nbase_url = \"http://old\"\n");
        assert!(upsert_provider(&path, &sample("new")).unwrap());

        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("# important header"), "comments were lost:\n{text}");
        assert!(text.contains("# previous provider"));
        assert!(text.contains("[[provider]]"));

        // And it is still valid for the app's data model.
        let f: ProvidersFile = toml::from_str(&text).expect("valid toml");
        assert_eq!(f.providers.len(), 2);
        let n = f.providers.iter().find(|p| p.id == "new").unwrap();
        assert_eq!(n.base_url.as_deref(), Some("https://api.sample.dev/v1"));
        assert_eq!(n.kind, ProviderKind::OpenaiChat);
        assert_eq!(n.models[0].context_window, Some(200_000));
        assert_eq!(n.models[0].pricing.unwrap().output, 7.5);
        assert_eq!(n.env["claude"]["ANTHROPIC_BASE_URL"], "{base_url}");
        assert_eq!(n.args["codex"], vec!["-c", "model={model}"]);
        // What was not touched stays intact.
        assert_eq!(f.providers[0].id, "old");
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn replaces_in_place_without_duplicating() {
        let path = file("schema = 1\n");
        upsert_provider(&path, &sample("one")).unwrap();
        upsert_provider(&path, &sample("two")).unwrap();

        let mut changed = sample("one");
        changed.base_url = Some("https://changed".into());
        changed.enabled = false;
        changed.models.clear();
        assert!(!upsert_provider(&path, &changed).unwrap(), "it was not new");

        let f: ProvidersFile = toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(f.providers.len(), 2);
        let one = f.providers.iter().find(|p| p.id == "one").unwrap();
        assert_eq!(one.base_url.as_deref(), Some("https://changed"));
        assert!(!one.enabled);
        assert!(one.models.is_empty());
        // Order is kept: «one» is still first.
        assert_eq!(f.providers[0].id, "one");
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn key_from_file_survives_the_round_trip() {
        let path = file("schema = 1\n");
        let mut p = sample("pi-like");
        p.api_key_env = None;
        p.api_key_file = Some("C:/Users/x/.pi/agent/auth.json".into());
        p.api_key_json_path = Some("gorouter.key".into());
        upsert_provider(&path, &p).unwrap();

        let f: ProvidersFile = toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let saved = &f.providers[0];
        assert_eq!(saved.api_key_file.as_deref(), Some("C:/Users/x/.pi/agent/auth.json"));
        assert_eq!(saved.api_key_json_path.as_deref(), Some("gorouter.key"));
        assert!(saved.api_key_env.is_none());
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn removes_a_provider() {
        let path = file("schema = 1\n");
        upsert_provider(&path, &sample("a")).unwrap();
        upsert_provider(&path, &sample("b")).unwrap();
        assert!(remove_provider(&path, "a").unwrap());
        assert!(!remove_provider(&path, "a").unwrap(), "already gone");

        let f: ProvidersFile = toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(f.providers.len(), 1);
        assert_eq!(f.providers[0].id, "b");
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn removing_does_not_lose_user_comments() {
        let path = file(
            "# file header\nschema = 1\n\n# important user comment\n[[provider]]\nid = \"one\"\n\n[[provider]]\nid = \"two\"\n",
        );
        assert!(remove_provider(&path, "one").unwrap());
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("# file header"), "{text}");
        assert!(
            text.contains("# important user comment"),
            "the comment should have moved to the next block:\n{text}"
        );
        let f: ProvidersFile = toml::from_str(&text).unwrap();
        assert_eq!(f.providers.len(), 1);
        assert_eq!(f.providers[0].id, "two");

        // Also when the removed one is the last.
        assert!(remove_provider(&path, "two").unwrap());
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("# important user comment"), "{text}");
        assert!(text.contains("# file header"));
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn works_on_the_factory_providers_file() {
        let path = file(crate::config::providers::DEFAULT_PROVIDERS_TOML);
        let before: ProvidersFile = toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();

        upsert_provider(&path, &sample("own")).unwrap();
        let mut anthropic = before.providers.iter().find(|p| p.id == "anthropic").unwrap().clone();
        anthropic.default_model = Some("claude-opus-4-6".into());
        upsert_provider(&path, &anthropic).unwrap();

        let text = std::fs::read_to_string(&path).unwrap();
        // The factory file's header comments are still there.
        assert!(text.contains("~/.sessions/providers.toml"));
        let after: ProvidersFile = toml::from_str(&text).unwrap();
        assert_eq!(after.providers.len(), before.providers.len() + 1);
        assert_eq!(
            after.providers.iter().find(|p| p.id == "anthropic").unwrap().default_model.as_deref(),
            Some("claude-opus-4-6")
        );
        // Models and their pricing survive the round trip.
        let a = after.providers.iter().find(|p| p.id == "anthropic").unwrap();
        assert!(a.models.iter().any(|m| m.id == "claude-sonnet-4-5" && m.context_window == Some(200_000)));
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn a_missing_file_is_created() {
        let path = std::env::temp_dir().join(format!("sessions-edit-new-{}.toml", uuid::Uuid::new_v4()));
        assert!(upsert_provider(&path, &sample("x")).unwrap());
        assert!(path.is_file());
        let f: ProvidersFile = toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(f.providers.len(), 1);
        std::fs::remove_file(path).ok();
    }
}
