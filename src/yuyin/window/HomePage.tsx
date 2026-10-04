// Yuyin fork: the home page — the slogan, what dictation has given the user
// (insights), the shortcuts, and what has left the machine (privacy).
import React, {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { useTranslation } from "react-i18next";
import { events } from "@/bindings";
import { useOsType } from "@/hooks/useOsType";
import { useSettings } from "@/hooks/useSettings";
import { yuyinApi, type YuyinStats } from "../api";
import { Card, Kbd } from "./ui";
import { LockIcon } from "./icons";
import { durationParts, formatCount, keyLabel, tapKeyLabel } from "./format";

/** api.deepseek.com → DeepSeek, openrouter.ai → OpenRouter; other hosts as
 * they are. */
export const serviceName = (host: string) =>
  host.endsWith("deepseek.com")
    ? "DeepSeek"
    : host === "openrouter.ai"
      ? "OpenRouter"
      : host;

const Big: React.FC<{ parts: { value: string; unit: string }[] }> = ({
  parts,
}) => (
  <div className="text-[26px] font-semibold tracking-[-0.02em] leading-tight text-text tabular-nums">
    {parts.map((p, i) => (
      <React.Fragment key={i}>
        {p.value}
        <span className="text-[13px] font-medium ms-[3px] me-[6px] last:me-0 whitespace-nowrap">
          {p.unit}
        </span>
      </React.Fragment>
    ))}
  </div>
);

const Insight: React.FC<{
  parts: { value: string; unit: string }[];
  label: string;
  hint?: string;
}> = ({ parts, label, hint }) => (
  <Card className="px-[18px] py-4 flex flex-col gap-1">
    <Big parts={parts} />
    <div className="text-[12px] text-muted" title={hint}>
      {label}
    </div>
  </Card>
);

const LEVEL_FILL = [
  "",
  "color-mix(in srgb, var(--color-logo-primary) 22%, transparent)",
  "color-mix(in srgb, var(--color-logo-primary) 42%, transparent)",
  "color-mix(in srgb, var(--color-logo-primary) 68%, transparent)",
  "var(--color-logo-primary)",
];
const level = (n: number) =>
  n === 0 ? 0 : n <= 2 ? 1 : n <= 5 ? 2 : n <= 9 ? 3 : 4;

/** One week column: a 10 px cell plus the 3 px gap. */
const WEEK_PX = 13;
/** The weekday label column (14 px) and the gap after it (6 px). */
const LABELS_PX = 20;
/** Room a month label needs ("10月", "Sep" at 10 px). */
const MONTH_LABEL_PX = 28;

const ActivityGrid: React.FC<{ stats: YuyinStats }> = ({ stats }) => {
  const { t, i18n } = useTranslation();
  const allWeeks = useMemo(() => {
    const out: ({ date: string; count: number } | null)[][] = [];
    for (let i = 0; i < stats.days.length; i += 7) {
      const week: ({ date: string; count: number } | null)[] = stats.days
        .slice(i, i + 7)
        .map((d) => ({ date: d.date, count: d.dictations }));
      while (week.length < 7) week.push(null);
      out.push(week);
    }
    return out;
  }, [stats.days]);

  // Show as many recent weeks as the card has room for. The card narrows
  // with the window, and on Windows with the system text size (WebView2
  // zooms the page by it), so a fixed half year spilled out of the card.
  const boxRef = useRef<HTMLDivElement>(null);
  const [width, setWidth] = useState(0);
  useLayoutEffect(() => {
    const box = boxRef.current;
    if (!box) return;
    const measure = () => setWidth(box.clientWidth);
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(box);
    return () => observer.disconnect();
  }, []);
  const fits = width
    ? Math.max(1, Math.floor((width - LABELS_PX + 3) / WEEK_PX))
    : allWeeks.length;
  const weeks = allWeeks.slice(-fits);

  const monthFmt = new Intl.DateTimeFormat(i18n.language, { month: "short" });
  const dayFmt = new Intl.DateTimeFormat(i18n.language, {
    month: "long",
    day: "numeric",
  });
  const months: { left: number; label: string }[] = [];
  weeks.forEach((w, i) => {
    const first = w[0];
    if (!first) return;
    const d = new Date(`${first.date}T12:00:00`);
    const prev = i > 0 ? weeks[i - 1][0] : null;
    const left = i * WEEK_PX;
    // A label that would run past the card is left out rather than wrapped.
    const room = !width || LABELS_PX + left + MONTH_LABEL_PX <= width;
    if (
      room &&
      (!prev || new Date(`${prev.date}T12:00:00`).getMonth() !== d.getMonth())
    ) {
      // The first column can hold the last days of a month; when the next
      // month starts right after, the two labels would overlap, so the
      // partial month gives way.
      const last = months[months.length - 1];
      if (last && left - last.left < MONTH_LABEL_PX) months.pop();
      months.push({ left, label: monthFmt.format(d) });
    }
  });

  return (
    <div ref={boxRef} className="flex flex-col gap-1.5 min-w-0">
      <div className="flex gap-1.5">
        <div className="w-3.5 flex flex-col gap-[3px] text-[9px] leading-[10px] text-muted">
          {[
            "",
            t("moqi.home.mon"),
            "",
            t("moqi.home.wed"),
            "",
            t("moqi.home.fri"),
            "",
          ].map((d, i) => (
            <span key={i} className="h-[10px]">
              {d}
            </span>
          ))}
        </div>
        <div className="flex gap-[3px]">
          {weeks.map((w, i) => (
            <div key={i} className="flex flex-col gap-[3px]">
              {w.map((d, j) =>
                d ? (
                  <div
                    key={j}
                    title={t("moqi.home.dayTooltip", {
                      date: dayFmt.format(new Date(`${d.date}T12:00:00`)),
                      count: d.count,
                    })}
                    className={`w-[10px] h-[10px] rounded-[2.5px] ${
                      d.count === 0 ? "bg-black/[0.07] dark:bg-white/10" : ""
                    }`}
                    style={
                      d.count
                        ? { background: LEVEL_FILL[level(d.count)] }
                        : undefined
                    }
                  />
                ) : (
                  <div
                    key={j}
                    className="w-[10px] h-[10px] rounded-[2.5px] shadow-[inset_0_0_0_1px_var(--color-hairline)]"
                  />
                ),
              )}
            </div>
          ))}
        </div>
      </div>
      <div className="flex items-center justify-between ps-5">
        <div className="relative h-3" style={{ width: weeks.length * WEEK_PX }}>
          {months.map((m) => (
            <span
              key={m.left}
              className="absolute top-0 text-[10px] leading-3 text-muted whitespace-nowrap"
              style={{ left: m.left }}
            >
              {m.label}
            </span>
          ))}
        </div>
      </div>
    </div>
  );
};

export const HomePage: React.FC<{
  onOpenHistory: () => void;
  onOpenSettings: () => void;
}> = ({ onOpenHistory, onOpenSettings }) => {
  const { t, i18n } = useTranslation();
  const os = useOsType();
  const { settings } = useSettings();
  const [stats, setStats] = useState<YuyinStats | null>(null);
  const [learn, setLearn] = useState<boolean | null | undefined>(undefined);

  const load = useCallback(() => {
    yuyinApi
      .stats()
      .then(setStats)
      .catch((e) => console.error("Failed to load stats:", e));
  }, []);

  useEffect(() => {
    yuyinApi
      .getConfig()
      .then((c) => setLearn(c.learn_from_edits))
      .catch(() => {});
  }, []);

  const answerLearn = async (on: boolean) => {
    setLearn(on);
    try {
      await yuyinApi.updateConfig({ learn_from_edits: on });
    } catch (e) {
      console.error("Failed to save the learning choice:", e);
    }
  };

  useEffect(() => {
    load();
    const unlisten = events.historyUpdatePayload.listen(() => load());
    window.addEventListener("focus", load);
    return () => {
      unlisten.then((fn) => fn());
      window.removeEventListener("focus", load);
    };
  }, [load]);

  const binding = settings?.bindings?.transcribe?.current_binding;
  const talkKey = keyLabel(binding, os, t);
  const tapKey = tapKeyLabel(binding, os, t);
  const lang = i18n.language;
  const s = stats;

  return (
    <div className="flex flex-col gap-[26px]">
      <div className="flex flex-col gap-1.5">
        <h1 className="m-0 text-[34px] font-bold tracking-[-0.025em] leading-tight text-text">
          {t("moqi.home.slogan")}
        </h1>
        <p className="m-0 text-[14px] text-muted">
          {t("settings.about.tagline")}
        </p>
      </div>

      {/* Ask once, after a few dictations (docs/隱私.md: off until asked). */}
      {learn === null && (s?.dictations ?? 0) >= 3 && (
        <Card className="px-[18px] py-4 flex items-center justify-between gap-5">
          <div className="flex flex-col gap-1 min-w-0">
            <h2 className="m-0 text-[13px] font-semibold text-text">
              {t("moqi.learn.askTitle")}
            </h2>
            <p className="m-0 text-[12px] leading-relaxed text-text/80">
              {t("moqi.learn.askBody")}
            </p>
          </div>
          <div className="flex flex-col gap-1.5 shrink-0">
            <button
              type="button"
              onClick={() => answerLearn(true)}
              className="text-[12px] font-medium px-3.5 py-[5px] rounded-[8px] bg-logo-primary text-white hover:brightness-110"
            >
              {t("moqi.learn.askYes")}
            </button>
            <button
              type="button"
              onClick={() => answerLearn(false)}
              className="text-[12px] px-3.5 py-[5px] rounded-[8px] bg-fill text-text hover:brightness-95"
            >
              {t("moqi.learn.askNo")}
            </button>
          </div>
        </Card>
      )}

      <div className="grid grid-cols-[minmax(0,1fr)_220px] gap-5 items-start">
        <div className="flex flex-col gap-3.5">
          <div className="flex items-baseline justify-between">
            <h2 className="m-0 text-[13px] font-semibold text-text">
              {t("moqi.home.insights")}
            </h2>
            <span className="text-[11px] text-muted">
              {t("moqi.home.computedLocally")}
            </span>
          </div>
          <div className="grid grid-cols-2 gap-3">
            <Insight
              parts={[
                {
                  value: formatCount(s?.chars ?? 0, lang),
                  unit: t("moqi.unit.chars", { count: s?.chars ?? 0 }),
                },
              ]}
              label={t("moqi.home.chars")}
            />
            <Insight
              parts={durationParts(s?.saved_ms ?? 0, t)}
              label={t("moqi.home.saved")}
              hint={t("moqi.home.savedHint")}
            />
            <Insight
              parts={[
                {
                  value: formatCount(s?.chars_per_minute ?? 0, lang),
                  unit: t("moqi.unit.charsPerMinute"),
                },
              ]}
              label={t("moqi.home.speed")}
            />
            <Insight
              parts={durationParts(s?.speaking_ms ?? 0, t)}
              label={t("moqi.home.speaking")}
            />
          </div>

          <Card className="px-[18px] py-4 flex flex-col gap-3.5">
            <div className="flex gap-7">
              {[
                [s?.active_days ?? 0, t("moqi.home.activeDays")],
                [s?.current_streak ?? 0, t("moqi.home.currentStreak")],
                [s?.longest_streak ?? 0, t("moqi.home.longestStreak")],
              ].map(([n, label]) => (
                <div key={String(label)}>
                  <span className="text-[20px] font-semibold text-text tabular-nums">
                    {n}
                  </span>
                  <span className="text-[12px] ms-[3px] text-text">
                    {t("moqi.unit.days", { count: Number(n) })}
                  </span>
                  <div className="text-[11px] text-muted mt-0.5">{label}</div>
                </div>
              ))}
            </div>
            {s && <ActivityGrid stats={s} />}
            {s && s.dictations === 0 && (
              <p className="m-0 text-[12px] text-muted">
                {t("moqi.home.empty", { key: talkKey })}
              </p>
            )}
          </Card>
        </div>

        <div className="flex flex-col gap-3.5 pt-[30px]">
          <Card className="p-4 flex flex-col gap-3">
            <h2 className="m-0 text-[13px] font-semibold text-text">
              {t("moqi.home.shortcuts")}
            </h2>
            <div className="flex flex-wrap items-center justify-between gap-x-2 gap-y-1">
              <span className="text-[12px] text-text/80 whitespace-nowrap">
                {t("moqi.home.holdToTalk")}
              </span>
              <span className="ml-auto">
                <Kbd>{talkKey}</Kbd>
              </span>
            </div>
            <div className="flex flex-wrap items-center justify-between gap-x-2 gap-y-1">
              <span className="text-[12px] text-text/80 whitespace-nowrap">
                {t("moqi.home.handsFree")}
              </span>
              <span className="ml-auto flex gap-[3px]">
                <Kbd>{tapKey}</Kbd>
                <Kbd>{tapKey}</Kbd>
              </span>
            </div>
            <div className="flex items-center justify-between gap-2">
              <span className="text-[12px] text-text/80">
                {t("moqi.home.cancel")}
              </span>
              <Kbd>{t("moqi.keys.esc")}</Kbd>
            </div>
            <button
              type="button"
              onClick={onOpenSettings}
              className="self-start text-[12px] text-logo-primary hover:underline"
            >
              {t("moqi.home.changeShortcut")}
            </button>
          </Card>

          <Card className="p-4 flex flex-col gap-3">
            <div className="flex items-center gap-[7px]">
              <LockIcon size={15} className="text-positive" />
              <h2 className="m-0 text-[13px] font-semibold text-text">
                {t("moqi.home.privacy")}
              </h2>
            </div>
            <div className="flex flex-col gap-[9px]">
              <div className="flex justify-between items-baseline gap-2">
                <span className="text-[12px] text-text/80">
                  {t("moqi.home.audioUploaded")}
                </span>
                <span className="text-[13px] font-semibold text-positive whitespace-nowrap">
                  {t("moqi.unit.secondsValue", { value: 0 })}
                </span>
              </div>
              <div className="flex justify-between items-baseline gap-2">
                <span className="text-[12px] text-text/80">
                  {t("moqi.home.appNamesSent")}
                </span>
                <span className="text-[13px] font-semibold text-positive whitespace-nowrap">
                  {t("moqi.unit.timesValue", { value: 0 })}
                </span>
              </div>
              <div className="h-px bg-hairline" />
              <div className="flex justify-between items-baseline gap-2">
                <span className="text-[12px] text-text/80">
                  {t("moqi.home.textSent")}
                </span>
                <span className="text-[13px] font-semibold text-text whitespace-nowrap">
                  {formatCount(s?.privacy.text_sent_chars ?? 0, lang)}{" "}
                  {t("moqi.unit.chars", {
                    count: s?.privacy.text_sent_chars ?? 0,
                  })}
                </span>
              </div>
              <div className="text-[11px] text-muted leading-relaxed">
                {s && s.privacy.sent_to.length > 0
                  ? t("moqi.home.sentNote", {
                      to: s.privacy.sent_to.map(serviceName).join("、"),
                    })
                  : t("moqi.home.nothingSent")}
              </div>
            </div>
            <button
              type="button"
              onClick={onOpenHistory}
              className="self-start text-[12px] text-logo-primary hover:underline"
            >
              {t("moqi.home.seeWhatWasSent")}
            </button>
          </Card>
        </div>
      </div>
    </div>
  );
};
