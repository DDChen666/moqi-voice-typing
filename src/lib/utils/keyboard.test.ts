import assert from "node:assert/strict";
import { formatKeyCombination, getKeyName } from "./keyboard";

const keyboardEvent = (value: { code?: string; key?: string }): KeyboardEvent =>
  value as KeyboardEvent;

const compoundKeys = [
  ["ScrollLock", "scrolllock", "Scroll Lock"],
  ["CapsLock", "capslock", "Caps Lock"],
  ["NumLock", "numlock", "Num Lock"],
  ["PageUp", "pageup", "Page Up"],
  ["PageDown", "pagedown", "Page Down"],
  ["PrintScreen", "printscreen", "Print Screen"],
] as const;

for (const [code, stored, displayed] of compoundKeys) {
  assert.equal(getKeyName(keyboardEvent({ code })), stored);
  assert.equal(formatKeyCombination(stored, "linux"), displayed);
}

assert.equal(getKeyName(keyboardEvent({ key: "CapsLock" })), "capslock");
assert.equal(
  getKeyName(keyboardEvent({ code: "AudioVolumeUp" })),
  "audiovolumeup",
);

// Yuyin fork: the Mac's Option key is Alt on Windows keyboards.
assert.equal(formatKeyCombination("option_right", "windows"), "Right Alt");
assert.equal(formatKeyCombination("alt_right", "windows"), "Right Alt");
assert.equal(formatKeyCombination("option_right", "macos"), "Right Option");
assert.equal(formatKeyCombination("option+space", "windows"), "Alt + Space");

console.log("keyboard: all assertions passed");
