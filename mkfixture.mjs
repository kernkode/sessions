import { copyFileSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";

const T = "C:/Users/KernKode/AppData/Local/Temp/sess-resume";
rmSync(T, { recursive: true, force: true });
mkdirSync(join(T, "state"), { recursive: true });
for (const f of ["config.toml", "agents.toml"]) {
  copyFileSync(join(homedir(), ".sessions", f), join(T, f));
}
const cwd = "C:/Users/KernKode/Desktop/my-roleplay";
writeFileSync(
  join(T, "state", "projects.json"),
  JSON.stringify(
    {
      projects: [{ id: "prj_r", name: "my-roleplay", path: cwd, created_at: 1786790000000, collapsed: false }],
      sessions: [
        {
          id: "ses_saved1",
          project_id: "prj_r",
          title: "Claude Code",
          agent_id: "claude",
          cwd,
          external_id: "1a3e965a-cfff-43b2-aff2-89a9b2a15264",
          created_at: 1786790000000,
          last_active_at: 1786790000000,
          status: "exited",
          exit_code: 0,
          pid: null,
          cols: 150,
          rows: 34,
          command_line: "claude.cmd",
        },
      ],
    },
    null,
    2,
  ),
);
console.log("fixture listo, cwd:", cwd);
