// Provider / agent compatibility rules. The backend computes them when the
// configuration loads (`Provider::supported_agents`), so here we only read them:
// the logic lives in a single place.
import type { Provider } from "./types";

export function supportsAgent(p: Provider, agentId: string): boolean {
  return p.enabled && p.supported_agents.includes(agentId);
}

export function providersFor(providers: Provider[], agentId: string): Provider[] {
  return providers.filter((p) => supportsAgent(p, agentId));
}

/** Scopes of `[provider.env.*]`: `all` plus one block per agent. */
export const SCOPE_ALL = "all";

export type KeySource = "literal" | "env" | "file" | "command" | "none";

export function keySourceOf(p: Provider): { source: KeySource; value: string } {
  if (p.api_key) return { source: "literal", value: p.api_key };
  if (p.api_key_file) return { source: "file", value: p.api_key_file };
  if (p.api_key_command) return { source: "command", value: p.api_key_command };
  if (p.api_key_env) return { source: "env", value: p.api_key_env };
  return { source: "none", value: "" };
}

function shorten(v: string, max = 38): string {
  return v.length > max ? `${v.slice(0, max)}…` : v;
}

export function describeKey(p: Provider): string {
  const { source, value } = keySourceOf(p);
  switch (source) {
    case "literal":
      return "escrita en el toml";
    case "file":
      return `fichero ${shorten(value)}${p.api_key_json_path ? ` → ${p.api_key_json_path}` : ""}`;
    case "command":
      return `comando ${shorten(value)}`;
    case "env":
      return `variable ${value}`;
    default:
      return "sin definir";
  }
}
