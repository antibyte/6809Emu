import { spawn } from "node:child_process";
import { existsSync, readFileSync, unlinkSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { setTimeout as sleep } from "node:timers/promises";
import { fileURLToPath } from "node:url";

const ROOT = fileURLToPath(new URL("..", import.meta.url));
const LOCK = join(ROOT, ".tauri-dev.lock");

function isRunning(pid) {
  try {
    process.kill(pid, 0);
    return true;
  } catch {
    return false;
  }
}

function acquireLock() {
  if (existsSync(LOCK)) {
    const pid = parseInt(readFileSync(LOCK, "utf8"), 10);
    if (!Number.isNaN(pid) && isRunning(pid)) {
      console.error(
        `Dev server already running (PID ${pid}). Stop it with Ctrl+C first.`,
      );
      process.exit(1);
    }
    unlinkSync(LOCK);
  }
  writeFileSync(LOCK, String(process.pid));
}

function releaseLock() {
  try {
    if (existsSync(LOCK)) {
      const pid = parseInt(readFileSync(LOCK, "utf8"), 10);
      if (pid === process.pid) {
        unlinkSync(LOCK);
      }
    }
  } catch {
    // ignore
  }
}

async function preparePort() {
  await import("./free-port.js");
  await sleep(300);
}

acquireLock();
process.on("exit", releaseLock);
process.on("SIGINT", () => process.exit(0));
process.on("SIGTERM", () => process.exit(0));

await preparePort();

const child = spawn("npx", ["tauri", "dev"], {
  stdio: "inherit",
  shell: true,
  cwd: ROOT,
});

child.on("exit", (code) => {
  releaseLock();
  process.exit(code ?? 1);
});