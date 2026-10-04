// Yuyin fork: new versions for Moqi's window. With "check for updates" on,
// Moqi asks GitHub once at launch and every six hours whether a newer
// release exists (it downloads only the version file); installing replaces
// the app and restarts it. Shared state, so the sidebar button and the
// settings row agree.
import { useEffect, useState } from "react";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";

export type UpdateState =
  | { kind: "idle" }
  | { kind: "checking" }
  | { kind: "latest" }
  | { kind: "available"; version: string }
  | { kind: "installing"; percent: number }
  | { kind: "failed"; message: string }
  /** Download or signature check failed; the old version stays. */
  | { kind: "installFailed"; version: string; message: string };

let state: UpdateState = { kind: "idle" };
let pending: Update | null = null;
const listeners = new Set<(s: UpdateState) => void>();

const set = (next: UpdateState) => {
  state = next;
  listeners.forEach((l) => l(next));
};

/** Ask for a newer version. Quiet failures for automatic checks: before the
 * first release there is no version file to find. */
export async function checkForUpdate(manual = false): Promise<UpdateState> {
  if (state.kind === "installing") return state;
  set({ kind: "checking" });
  try {
    pending = await check();
    set(
      pending
        ? { kind: "available", version: pending.version }
        : { kind: "latest" },
    );
  } catch (e) {
    set(manual ? { kind: "failed", message: String(e) } : { kind: "idle" });
  }
  return state;
}

/** Download, install and restart. */
export async function installUpdate() {
  if (!pending) return;
  const version = pending.version;
  let total = 0;
  let done = 0;
  set({ kind: "installing", percent: 0 });
  try {
    await pending.downloadAndInstall((event) => {
      if (event.event === "Started") total = event.data.contentLength ?? 0;
      if (event.event === "Progress") {
        done += event.data.chunkLength;
        set({
          kind: "installing",
          percent: total ? Math.min(100, Math.round((done / total) * 100)) : 0,
        });
      }
    });
    await relaunch();
  } catch (e) {
    set({ kind: "installFailed", version, message: String(e) });
  }
}

export function useUpdateState() {
  const [s, setS] = useState(state);
  useEffect(() => {
    listeners.add(setS);
    return () => {
      listeners.delete(setS);
    };
  }, []);
  return s;
}

const EVERY_MS = 6 * 60 * 60 * 1000;

/** Automatic checks while `enabled`: now, then every six hours. */
export function useAutomaticUpdateChecks(enabled: boolean) {
  useEffect(() => {
    if (!enabled) return;
    checkForUpdate();
    const id = setInterval(() => checkForUpdate(), EVERY_MS);
    return () => clearInterval(id);
  }, [enabled]);
}
