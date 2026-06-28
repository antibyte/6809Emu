import { spawn } from "node:child_process";
import { fileURLToPath } from "node:url";

const args = process.argv.slice(2);

if (args[0] === "dev") {
  await import("./tauri-dev.js");
} else {
  const child = spawn("npx", ["tauri", ...args], {
    stdio: "inherit",
    shell: true,
    cwd: fileURLToPath(new URL("..", import.meta.url)),
  });
  child.on("exit", (code) => process.exit(code ?? 1));
}