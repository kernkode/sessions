// Frees the dev server port when a previous Vite instance was left running.
//   node scripts/free-port.mjs [port]
//
// Only kills the process if it is a Node one holding that port: never touches
// anything else. Used by `npm run dev:free` and by `predev` so `npm run app:dev`
// does not fail because of a leftover server.
import { execFileSync } from "node:child_process";

const port = process.argv[2] ?? "5273";
const isWindows = process.platform === "win32";

/** PIDs listening on the port. */
function listeners() {
  try {
    if (isWindows) {
      const out = execFileSync("netstat", ["-ano"], { encoding: "utf8" });
      return [
        ...new Set(
          out
            .split(/\r?\n/)
            .filter((l) => /LISTENING/.test(l) && new RegExp(`[:.]${port}\\s`).test(l))
            .map((l) => l.trim().split(/\s+/).pop())
            .filter((p) => p && p !== "0"),
        ),
      ];
    }
    const out = execFileSync("lsof", ["-ti", `tcp:${port}`, "-sTCP:LISTEN"], { encoding: "utf8" });
    return [...new Set(out.split(/\s+/).filter(Boolean))];
  } catch {
    return [];
  }
}

/** Process image name, to avoid killing something unrelated. */
function imageName(pid) {
  try {
    if (isWindows) {
      const out = execFileSync("tasklist", ["/FI", `PID eq ${pid}`, "/FO", "CSV", "/NH"], {
        encoding: "utf8",
      });
      return (out.split('","')[0] ?? "").replace(/^"/, "").toLowerCase();
    }
    return execFileSync("ps", ["-p", pid, "-o", "comm="], { encoding: "utf8" }).trim().toLowerCase();
  } catch {
    return "";
  }
}

const pids = listeners();
if (pids.length === 0) {
  process.exit(0);
}

for (const pid of pids) {
  const name = imageName(pid);
  if (!/node/.test(name)) {
    console.log(
      `puerto ${port} ocupado por ${name || "un proceso desconocido"} (pid ${pid}); no lo toco`,
    );
    continue;
  }
  try {
    if (isWindows) execFileSync("taskkill", ["/F", "/PID", pid], { stdio: "ignore" });
    else execFileSync("kill", ["-9", pid], { stdio: "ignore" });
    console.log(`liberado el puerto ${port} (${name}, pid ${pid})`);
  } catch (e) {
    console.log(`no pude liberar el puerto ${port} (pid ${pid}): ${e.message}`);
  }
}
