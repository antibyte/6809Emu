import { check, type Update, type DownloadEvent } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { getVersion } from "@tauri-apps/api/app";

const SKIP_KEY = "update.skipVersion";

export type UpdateOffer = {
  update: Update;
  version: string;
  currentVersion: string;
  body: string | null;
  date: string | undefined;
};

export async function getAppVersion(): Promise<string> {
  try {
    return await getVersion();
  } catch {
    return "0.0.0";
  }
}

export function isSkipped(version: string): boolean {
  if (typeof localStorage === "undefined") return false;
  return localStorage.getItem(SKIP_KEY) === version;
}

export function skipVersion(version: string): void {
  if (typeof localStorage === "undefined") return;
  localStorage.setItem(SKIP_KEY, version);
}

export function clearSkip(): void {
  if (typeof localStorage === "undefined") return;
  localStorage.removeItem(SKIP_KEY);
}

/**
 * Check GitHub Releases (via latest.json) for a newer app version.
 * @param silent When true, skip versions the user dismissed and swallow network errors.
 */
export async function checkForUpdates(opts?: {
  silent?: boolean;
}): Promise<UpdateOffer | null> {
  const silent = opts?.silent ?? false;

  // In Vite dev the updater is rarely useful and may error without a signed build.
  if (import.meta.env.DEV && silent) {
    return null;
  }

  try {
    const update = await check();
    if (!update) return null;

    if (silent && isSkipped(update.version)) {
      return null;
    }

    return {
      update,
      version: update.version,
      currentVersion: update.currentVersion,
      body: update.body ?? null,
      date: update.date,
    };
  } catch (err) {
    if (!silent) throw err;
    console.warn("Update check failed:", err);
    return null;
  }
}

export async function downloadAndInstall(
  update: Update,
  onProgress?: (percent: number | null) => void
): Promise<void> {
  let downloaded = 0;
  let contentLength: number | undefined;

  await update.downloadAndInstall((event: DownloadEvent) => {
    switch (event.event) {
      case "Started":
        contentLength = event.data.contentLength ?? undefined;
        downloaded = 0;
        onProgress?.(contentLength ? 0 : null);
        break;
      case "Progress":
        downloaded += event.data.chunkLength;
        if (contentLength && contentLength > 0) {
          onProgress?.(Math.min(100, Math.round((downloaded / contentLength) * 100)));
        } else {
          onProgress?.(null);
        }
        break;
      case "Finished":
        onProgress?.(100);
        break;
    }
  });

  // On Windows the installer exits the app; relaunch still helps on other OSes.
  try {
    await relaunch();
  } catch (err) {
    console.warn("Relaunch after update failed:", err);
  }
}
