// Yuyin fork: the window's name in the interface language. Windows shows it
// in the title bar and on the taskbar button ("默契" rather than "Moqi" for
// Chinese); macOS hides the title bar and names the app from its bundle.
import { getCurrentWindow } from "@tauri-apps/api/window";
import { platform } from "@tauri-apps/plugin-os";
import i18n from "@/i18n";

export const syncWindowTitle = (): void => {
  let os = "";
  try {
    os = platform();
  } catch {
    return;
  }
  if (os !== "windows") return;
  const apply = () => {
    getCurrentWindow()
      .setTitle(i18n.t("appName"))
      .catch(() => {});
  };
  apply();
  i18n.on("languageChanged", apply);
};
