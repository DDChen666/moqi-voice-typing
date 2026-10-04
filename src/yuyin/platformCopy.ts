// Yuyin fork: platform wording. The base strings describe the Mac ("右邊的
// Option 鍵", 鑰匙圈, 選單列). Each locale may add a top-level `windows`
// block that mirrors the paths it rewords; on Windows those values replace
// the base ones before i18next starts, so every screen (ours and Handy's)
// shows the Windows wording without per-call-site checks.
import { platform } from "@tauri-apps/plugin-os";

type Tree = Record<string, unknown>;

const isTree = (v: unknown): v is Tree =>
  typeof v === "object" && v !== null && !Array.isArray(v);

/** `base` with every leaf of `patch` written over it (both left untouched). */
export const overlay = (base: Tree, patch: Tree): Tree => {
  const out: Tree = { ...base };
  for (const [key, value] of Object.entries(patch)) {
    out[key] =
      isTree(value) && isTree(base[key])
        ? overlay(base[key] as Tree, value)
        : value;
  }
  return out;
};

const currentPlatform = (): string => {
  try {
    return platform();
  } catch {
    // Outside Tauri (browser previews, tests): keep the base wording.
    return "";
  }
};

/**
 * Apply the running platform's wording to each locale's translation tree.
 * Platform blocks are dropped from the result so they never show up as keys.
 */
export const applyPlatformCopy = (
  resources: Record<string, { translation: Tree }>,
  os: string = currentPlatform(),
): void => {
  for (const res of Object.values(resources)) {
    const { windows, ...base } = res.translation;
    res.translation =
      os === "windows" && isTree(windows) ? overlay(base, windows) : base;
  }
};
