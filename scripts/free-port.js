import { execSync } from "node:child_process";
import { platform } from "node:os";
import { setTimeout as sleep } from "node:timers/promises";

const PORT = 1420;

async function freePortWindows() {
  try {
    const output = execSync(`netstat -ano | findstr :${PORT}`, {
      encoding: "utf8",
      stdio: ["ignore", "pipe", "ignore"],
    });

    const pids = new Set();
    for (const line of output.split(/\r?\n/)) {
      if (!line.includes(`:${PORT}`)) continue;
      if (/LISTENING|ABH/i.test(line)) {
        const parts = line.trim().split(/\s+/);
        const pid = parts[parts.length - 1];
        if (/^\d+$/.test(pid)) {
          pids.add(pid);
        }
      }
    }

    for (const pid of pids) {
      try {
        execSync(`taskkill /F /PID ${pid}`, { stdio: "ignore" });
        console.log(`Freed port ${PORT} (stopped PID ${pid})`);
      } catch {
        // Already gone.
      }
    }

    if (pids.size > 0) {
      await sleep(400);
    }
  } catch {
    // Port not in use.
  }
}

if (platform() === "win32") {
  await freePortWindows();
}