// opencode plugin: per-session wire identities.
// Install: copy (or symlink) this file into ~/.config/opencode/plugin/ —
// opencode auto-loads every *.js there; no opencode.json entry needed.
// opencode forwards no session-id env var to spawned MCP servers
// (SlanchaAi/wire#92). opencode fires session.created ~0.5s BEFORE booting
// local MCP servers, so the event hook learns the session id in time to
// stamp WIRE_SESSION_ID=opencode-<sessionID> — birth identity == resume
// identity for every session.
// Priority: exported WIRE_SESSION_ID > first top-level session.created >
// `-s <id>` / `-c` argv resolution > fresh uuid. The lookup only ever
// yields the correct or a fresh key, never a foreign session's identity.
import { randomUUID } from "node:crypto"
import { execFileSync } from "node:child_process"
import { existsSync, realpathSync } from "node:fs"
import os from "node:os"
import path from "node:path"

function explicitSessionId(argv) {
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i]
    if ((a === "-s" || a === "--session") && argv[i + 1] && !argv[i + 1].startsWith("-")) {
      return argv[i + 1]
    }
    if (a.startsWith("--session=")) return a.slice("--session=".length)
  }
  return null
}

function tryRealpath(p) {
  try {
    return realpathSync(p)
  } catch {
    return p
  }
}

function lastSessionId(cwd) {
  try {
    const db = path.join(
      process.env.XDG_DATA_HOME || path.join(os.homedir(), ".local", "share"),
      "opencode",
      "opencode.db",
    )
    if (!existsSync(db)) return null
    const esc = (s) => s.replace(/'/g, "''")
    const dirs = [...new Set([cwd, tryRealpath(cwd)])].map(esc).join("','")
    const out = execFileSync(
      "sqlite3",
      [
        "-readonly",
        db,
        `select id from session where directory in ('${dirs}') and parent_id is null order by time_updated desc limit 1;`,
      ],
      { encoding: "utf8", timeout: 2000 },
    )
    const id = out.trim()
    return /^ses_/.test(id) ? id : null
  } catch {
    return null
  }
}

export default async () => {
  let wireCfg = null
  let stampedByEvent = false

  return {
    config: (cfg) => {
      const wire = cfg.mcp?.wire
      if (!wire || wire.type !== "local") return
      wireCfg = wire
      if (process.env.WIRE_SESSION_ID) return
      const argv = process.argv
      let key = explicitSessionId(argv)
      if (!key && argv.some((a) => a === "-c" || a === "--continue")) {
        if (!argv.includes("--fork")) key = lastSessionId(process.cwd())
      }
      wire.environment = {
        ...wire.environment,
        WIRE_SESSION_ID: key ? `opencode-${key}` : `opencode-${randomUUID()}`,
      }
    },
    event: ({ event }) => {
      if (stampedByEvent || !wireCfg || process.env.WIRE_SESSION_ID) return
      if (event?.type !== "session.created" && event?.type !== "session.updated") return
      const info = event?.properties?.info
      if (!info?.id || !/^ses_/.test(info.id)) return
      if (info.parentID) return
      // For resume flows trust only the id argv already named, if any.
      const argvKey = explicitSessionId(process.argv)
      if (argvKey && info.id !== argvKey) return
      wireCfg.environment = {
        ...wireCfg.environment,
        WIRE_SESSION_ID: `opencode-${info.id}`,
      }
      stampedByEvent = true
    },
  }
}
