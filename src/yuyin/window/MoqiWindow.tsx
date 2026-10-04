// Yuyin fork: the 1.0 main window (design:
// https://claude.ai/artifact/CcyDKSUUKAkAwsPq5siWFA). Three pages in the
// sidebar — home, history, dictionary — and settings behind the gear,
// replacing Handy's settings sidebar.
import React, { useEffect, useLayoutEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { getVersion } from "@tauri-apps/api/app";
import {
  installUpdate,
  useAutomaticUpdateChecks,
  useUpdateState,
} from "../update";
import AccessibilityPermissions from "@/components/AccessibilityPermissions";
import SecureInputWarning from "@/components/SecureInputWarning";
import {
  DebugSettings,
  type OnboardingPreviewStep,
} from "@/components/settings";
import { useSettings } from "@/hooks/useSettings";
import { useOsType } from "@/hooks/useOsType";
import { YuyinMark } from "../YuyinLogo";
import { HomePage } from "./HomePage";
import { HistoryPage } from "./HistoryPage";
import { DictionaryPage } from "./DictionaryPage";
import { SettingsPage } from "./SettingsPage";
import { BookIcon, ClockIcon, FlaskIcon, GearIcon, HomeIcon } from "./icons";

export type Page = "home" | "history" | "dictionary" | "settings" | "debug";

const NavItem: React.FC<{
  icon: React.ReactNode;
  label: string;
  active: boolean;
  onClick: () => void;
}> = ({ icon, label, active, onClick }) => (
  <button
    type="button"
    onClick={onClick}
    aria-current={active ? "page" : undefined}
    className={`flex items-center gap-2.5 w-full px-2.5 py-[6px] rounded-[8px] text-[13px] text-start text-text transition-colors ${
      active
        ? "bg-black/[0.075] dark:bg-white/[0.12] font-semibold"
        : "hover:bg-black/[0.04] dark:hover:bg-white/[0.06]"
    }`}
  >
    {icon}
    <span className="truncate">{label}</span>
  </button>
);

export const MoqiWindow: React.FC<{
  onPreviewOnboarding: (step: OnboardingPreviewStep) => void;
  /** Which page opens first (home unless a caller asks otherwise). */
  initialPage?: Page;
}> = ({ onPreviewOnboarding, initialPage = "home" }) => {
  const { t, i18n } = useTranslation();
  const { settings } = useSettings();
  // The macOS title bar is transparent, so the sidebar leaves room for the
  // traffic lights; other platforms keep their own title bar.
  const macTitleBar = useOsType() === "macos";
  const [page, setPage] = useState<Page>(initialPage);
  const [version, setVersion] = useState("");
  const scrollRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    getVersion()
      .then(setVersion)
      .catch(() => {});
  }, []);

  useLayoutEffect(() => {
    scrollRef.current?.scrollTo({ top: 0 });
  }, [page]);

  // New versions: checked at launch and every six hours, unless turned off.
  useAutomaticUpdateChecks(settings?.update_checks_enabled ?? false);
  const update = useUpdateState();

  const debug = settings?.debug_mode ?? false;
  useEffect(() => {
    if (!debug && page === "debug") setPage("home");
  }, [debug, page]);

  return (
    <div dir={i18n.dir()} className="h-screen flex select-none cursor-default">
      <aside className="moqi-sidebar w-[200px] shrink-0 h-full flex flex-col border-e border-hairline px-2.5 pb-3.5">
        <div
          data-tauri-drag-region
          className={`${macTitleBar ? "h-[52px]" : "h-5"} shrink-0`}
        />
        <div className="flex items-center gap-[9px] px-2.5 pb-[18px]">
          <YuyinMark size={26} />
          <span className="text-[17px] font-semibold tracking-[-0.01em] text-text">
            {t("appName")}
          </span>
        </div>
        <nav aria-label={t("moqi.nav.label")} className="flex flex-col gap-0.5">
          <NavItem
            icon={<HomeIcon />}
            label={t("moqi.nav.home")}
            active={page === "home"}
            onClick={() => setPage("home")}
          />
          <NavItem
            icon={<ClockIcon />}
            label={t("moqi.nav.history")}
            active={page === "history"}
            onClick={() => setPage("history")}
          />
          <NavItem
            icon={<BookIcon />}
            label={t("moqi.nav.dictionary")}
            active={page === "dictionary"}
            onClick={() => setPage("dictionary")}
          />
        </nav>
        <div className="flex-1" />
        {debug && (
          <NavItem
            icon={<FlaskIcon />}
            label={t("sidebar.debug")}
            active={page === "debug"}
            onClick={() => setPage("debug")}
          />
        )}
        <NavItem
          icon={<GearIcon />}
          label={t("moqi.nav.settings")}
          active={page === "settings"}
          onClick={() => setPage("settings")}
        />
        {(update.kind === "available" ||
          update.kind === "installing" ||
          update.kind === "installFailed") && (
          <button
            type="button"
            onClick={installUpdate}
            disabled={update.kind === "installing"}
            className="mx-1 mt-2 px-2.5 py-1.5 rounded-[8px] text-start text-[12px] font-medium bg-logo-primary text-white hover:brightness-110 disabled:opacity-80"
          >
            {update.kind === "installing"
              ? t("moqi.update.installing", { percent: update.percent })
              : update.kind === "installFailed"
                ? t("moqi.update.retry")
                : t("moqi.update.available", { version: update.version })}
          </button>
        )}
        {version && (
          <div className="px-2.5 pt-2 text-[11px] text-muted">
            {t("settings.about.versionLabel", { version })}
          </div>
        )}
      </aside>

      <main className="flex-1 min-w-0 relative bg-background">
        <div
          data-tauri-drag-region
          className="absolute inset-x-0 top-0 h-10 z-10"
        />
        <div ref={scrollRef} className="h-full overflow-y-auto">
          <div className="px-10 pt-10 pb-8 flex flex-col gap-4">
            <AccessibilityPermissions />
            <SecureInputWarning />
            {page === "home" && (
              <HomePage
                onOpenHistory={() => setPage("history")}
                onOpenSettings={() => setPage("settings")}
              />
            )}
            {page === "history" && <HistoryPage />}
            {page === "dictionary" && <DictionaryPage />}
            {page === "settings" && <SettingsPage />}
            {page === "debug" && (
              <DebugSettings onPreviewOnboarding={onPreviewOnboarding} />
            )}
          </div>
        </div>
      </main>
    </div>
  );
};
