// Yuyin fork: setup step 2 — keep the words as spoken (all local), or let
// Moqi tidy them with the user's own DeepSeek key.
import React, { useState } from "react";
import { useTranslation } from "react-i18next";
import { openUrl } from "@tauri-apps/plugin-opener";
import { yuyinApi } from "../api";
import { LockIcon, SparkleIcon } from "../window/icons";
import { PrimaryButton, SmallButton, TextInput } from "../window/ui";

const Dots: React.FC<{ step: number }> = ({ step }) => {
  const { t } = useTranslation();
  return (
    <div
      className="fixed top-[22px] end-6 flex gap-1.5"
      aria-label={t("moqi.onboarding.step", { n: step, total: 3 })}
    >
      {[1, 2, 3].map((n) => (
        <span
          key={n}
          className={`w-[18px] h-1 rounded-sm ${n <= step ? "bg-text" : "bg-black/15 dark:bg-white/20"}`}
        />
      ))}
    </div>
  );
};
export { Dots as StepDots };

const Choice: React.FC<{
  selected: boolean;
  onSelect: () => void;
  icon: React.ReactNode;
  title: string;
  badge?: string;
  summary: string;
  points: string[];
}> = ({ selected, onSelect, icon, title, badge, summary, points }) => (
  <button
    type="button"
    role="radio"
    aria-checked={selected}
    onClick={onSelect}
    className={`text-start p-[18px] rounded-[14px] bg-surface flex flex-col gap-2.5 transition-shadow ${
      selected
        ? "shadow-[0_0_0_2px_var(--color-logo-primary),0_4px_14px_rgba(0,113,227,0.12)]"
        : "shadow-[0_0_0_0.5px_var(--color-hairline),0_1px_3px_rgba(0,0,0,0.05)]"
    }`}
  >
    <span className="flex items-center gap-2">
      {icon}
      <span className="text-[15px] font-semibold text-text">{title}</span>
      {badge && (
        <span className="text-[11px] font-semibold text-logo-primary px-1.5 py-px rounded-[5px] bg-logo-primary/10">
          {badge}
        </span>
      )}
    </span>
    <span className="text-[13px] leading-normal text-text/80">{summary}</span>
    <span className="flex flex-col gap-1 text-[12px] text-muted">
      {points.map((p) => (
        <span key={p} className="flex items-baseline gap-1.5">
          <span
            aria-hidden="true"
            className="w-1 h-1 rounded-full bg-current shrink-0 translate-y-[-2px]"
          />
          {p}
        </span>
      ))}
    </span>
  </button>
);

export const PolishStep: React.FC<{
  onDone: () => void;
  preview?: boolean;
}> = ({ onDone, preview = false }) => {
  const { t } = useTranslation();
  const [choice, setChoice] = useState<"raw" | "tidy">("tidy");
  const [key, setKey] = useState("");
  const [testing, setTesting] = useState(false);
  const [result, setResult] = useState<{ ok: boolean; text: string } | null>(
    null,
  );

  const saveKey = async () => {
    // This step offers DeepSeek only, so the key is DeepSeek's.
    if (key.trim())
      await yuyinApi.setApiKey(key.trim(), "https://api.deepseek.com");
  };

  const test = async () => {
    setTesting(true);
    setResult(null);
    try {
      await saveKey();
      setResult({
        ok: true,
        text: await yuyinApi.testPolish(t("yuyin.test.sample")),
      });
    } catch (e) {
      setResult({ ok: false, text: String(e) });
    } finally {
      setTesting(false);
    }
  };

  /** 整理 without a key would fail every time, so it falls back to 原話. */
  const finish = async (level: "raw" | "tidy") => {
    if (!preview) {
      try {
        if (level === "tidy") await saveKey();
        const config = await yuyinApi.getConfig();
        await yuyinApi.setConfig({ ...config, level });
      } catch (e) {
        console.error("Failed to save the clean-up choice:", e);
      }
    }
    onDone();
  };

  const tidy = choice === "tidy";

  return (
    <div className="h-screen w-full flex flex-col items-center justify-center p-8 bg-background">
      <div data-tauri-drag-region className="fixed top-0 inset-x-0 h-[52px]" />
      <Dots step={2} />
      <div className="w-full max-w-[600px] flex flex-col items-center">
        <h1 className="m-0 text-[26px] font-bold tracking-[-0.02em] text-text">
          {t("moqi.onboarding.polishTitle")}
        </h1>
        <p className="mt-2 mb-0 text-[14px] text-muted">
          {t("moqi.onboarding.polishSubtitle")}
        </p>

        <div role="radiogroup" className="mt-7 w-full grid grid-cols-2 gap-3.5">
          <Choice
            selected={!tidy}
            onSelect={() => setChoice("raw")}
            icon={<LockIcon className="text-positive" />}
            title={t("moqi.levels.raw")}
            summary={t("moqi.onboarding.rawSummary")}
            points={[
              t("moqi.onboarding.rawPoint1"),
              t("moqi.onboarding.rawPoint2"),
              t("moqi.onboarding.rawPoint3"),
            ]}
          />
          <Choice
            selected={tidy}
            onSelect={() => setChoice("tidy")}
            icon={<SparkleIcon className="text-logo-primary" />}
            title={t("moqi.levels.tidy")}
            badge={t("moqi.onboarding.recommended")}
            summary={t("moqi.onboarding.tidySummary")}
            points={[
              t("moqi.onboarding.tidyPoint1"),
              t("moqi.onboarding.tidyPoint2"),
              t("moqi.onboarding.tidyPoint3"),
            ]}
          />
        </div>

        {tidy && (
          <div className="mt-4 w-full flex flex-col gap-2">
            <div className="flex gap-2">
              <TextInput
                type="password"
                value={key}
                onChange={(e) => setKey(e.target.value)}
                placeholder={t("moqi.onboarding.keyPlaceholder")}
                aria-label={t("moqi.onboarding.keyPlaceholder")}
                className="flex-1"
              />
              <SmallButton
                onClick={test}
                disabled={!key.trim() || testing || preview}
                className="!px-3.5"
              >
                {testing ? t("moqi.settings.testing") : t("moqi.settings.test")}
              </SmallButton>
            </div>
            {result && (
              <p
                className={`m-0 text-[12px] select-text ${result.ok ? "text-positive" : "text-error"}`}
              >
                {result.ok
                  ? t("moqi.settings.testResult", { text: result.text })
                  : result.text}
              </p>
            )}
            <span className="text-[11px] text-muted">
              {t("moqi.onboarding.keyNote")}{" "}
              <button
                type="button"
                onClick={() =>
                  openUrl("https://platform.deepseek.com/api_keys")
                }
                className="text-logo-primary hover:underline"
              >
                {t("moqi.onboarding.getKey")}
              </button>
            </span>
          </div>
        )}

        <PrimaryButton
          className="mt-7"
          onClick={() => finish(tidy && key.trim() ? "tidy" : "raw")}
          disabled={tidy && !key.trim()}
        >
          {t("moqi.onboarding.continue")}
        </PrimaryButton>
        {tidy && (
          <button
            type="button"
            onClick={() => finish("raw")}
            className="mt-2.5 text-[12px] text-logo-primary hover:underline"
          >
            {t("moqi.onboarding.skip")}
          </button>
        )}
      </div>
    </div>
  );
};
