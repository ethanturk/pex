import { signal } from "@preact/signals";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";

export type UpdateState =
  | { kind: "idle" }
  | { kind: "checking" }
  | { kind: "available"; update: Update }
  | { kind: "downloading"; update: Update; downloaded: number; total: number | null }
  | { kind: "ready" } // downloaded + installed; waiting for restart
  | { kind: "error"; message: string };

export const updateState = signal<UpdateState>({ kind: "idle" });

let started = false;

// One check per app session is plenty — users will quit and reopen often
// enough that we don't need to poll.
export async function startUpdateCheck() {
  if (started) return;
  started = true;
  updateState.value = { kind: "checking" };
  try {
    const update = await check();
    if (update) {
      updateState.value = { kind: "available", update };
    } else {
      updateState.value = { kind: "idle" };
    }
  } catch (e) {
    // Network errors are routine (offline, GitHub blip) — don't pop UI for them.
    console.warn("[updater] check failed:", e);
    updateState.value = { kind: "idle" };
  }
}

export async function recheckForUpdate() {
  started = false;
  await startUpdateCheck();
}

export async function downloadAndInstall() {
  const s = updateState.value;
  if (s.kind !== "available") return;
  const update = s.update;
  updateState.value = { kind: "downloading", update, downloaded: 0, total: null };

  try {
    await update.downloadAndInstall((event) => {
      const cur = updateState.value;
      if (cur.kind !== "downloading") return;
      if (event.event === "Started") {
        updateState.value = {
          kind: "downloading",
          update,
          downloaded: 0,
          total: event.data.contentLength ?? null,
        };
      } else if (event.event === "Progress") {
        updateState.value = {
          kind: "downloading",
          update,
          downloaded: cur.downloaded + event.data.chunkLength,
          total: cur.total,
        };
      } else if (event.event === "Finished") {
        updateState.value = { kind: "ready" };
      }
    });
    // Restart to pick up the new bundle. Tauri's `relaunch` quits cleanly and
    // re-execs from the new binary path.
    await relaunch();
  } catch (e) {
    updateState.value = { kind: "error", message: String(e) };
  }
}

export function dismissUpdate() {
  updateState.value = { kind: "idle" };
}
