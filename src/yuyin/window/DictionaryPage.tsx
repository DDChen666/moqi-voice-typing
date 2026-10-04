// Yuyin fork: the personal dictionary — names and terms the clean-up must
// spell the user's way. Stored in yuyin.json (config.vocab). Below it, the
// corrections learned from the user's edits (learn.rs).
import React, { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import {
  yuyinApi,
  type LearnedRule,
  type Snippet,
  type YuyinConfig,
} from "../api";
import { Card, PageTitle, SmallButton, TextInput } from "./ui";

/** Voice snippets: say the trigger, get the text (snippets.rs). */
const SnippetsCard: React.FC<{
  snippets: Snippet[];
  change: (edit: (s: Snippet[]) => Snippet[]) => void;
}> = ({ snippets, change }) => {
  const { t } = useTranslation();
  const [trigger, setTrigger] = useState("");
  const [text, setText] = useState("");
  const add = () => {
    const tr = trigger.trim();
    if (!tr || !text.trim()) return;
    change((list) => [
      ...list.filter((s) => s.trigger !== tr),
      { trigger: tr, text },
    ]);
    setTrigger("");
    setText("");
  };
  return (
    <Card className="p-4 flex flex-col gap-3">
      <div className="flex justify-between items-baseline gap-3">
        <span className="text-[12px] font-semibold text-text">
          {t("moqi.snippets.title")}
        </span>
        <span className="text-[11px] text-muted">
          {t("moqi.snippets.hint")}
        </span>
      </div>
      {snippets.length === 0 ? (
        <p className="m-0 text-[12px] leading-relaxed text-muted">
          {t("moqi.snippets.empty")}
        </p>
      ) : (
        <ul className="m-0 p-0 list-none flex flex-col divide-y divide-hairline">
          {snippets.map((s) => (
            <li
              key={s.trigger}
              className="flex items-start justify-between gap-3 py-2"
            >
              <span className="min-w-0 flex flex-col gap-0.5">
                <span className="text-[13px] font-medium text-text select-text">
                  {s.trigger}
                </span>
                <span className="text-[12px] text-muted whitespace-pre-wrap break-words select-text">
                  {s.text}
                </span>
              </span>
              <SmallButton
                onClick={() =>
                  change((list) => list.filter((x) => x.trigger !== s.trigger))
                }
              >
                {t("moqi.snippets.remove")}
              </SmallButton>
            </li>
          ))}
        </ul>
      )}
      <form
        className="flex flex-col gap-2"
        onSubmit={(e) => {
          e.preventDefault();
          add();
        }}
      >
        <TextInput
          value={trigger}
          onChange={(e) => setTrigger(e.target.value)}
          placeholder={t("moqi.snippets.triggerPlaceholder")}
          aria-label={t("moqi.snippets.triggerPlaceholder")}
        />
        <textarea
          value={text}
          onChange={(e) => setText(e.target.value)}
          placeholder={t("moqi.snippets.textPlaceholder")}
          aria-label={t("moqi.snippets.textPlaceholder")}
          rows={3}
          className="text-[13px] px-3 py-[7px] rounded-[9px] bg-surface text-text shadow-[0_0_0_0.5px_rgba(0,0,0,0.16)] dark:shadow-[0_0_0_0.5px_rgba(255,255,255,0.16)] outline-none focus:shadow-[0_0_0_2px_var(--color-logo-primary)] placeholder:text-muted resize-y"
        />
        <button
          type="submit"
          disabled={!trigger.trim() || !text.trim()}
          className="self-end text-[13px] font-medium px-[18px] py-[6px] rounded-[9px] bg-logo-primary text-white disabled:opacity-40 hover:brightness-110 transition"
        >
          {t("moqi.snippets.add")}
        </button>
      </form>
    </Card>
  );
};

const LearnedList: React.FC<{ enabled: boolean }> = ({ enabled }) => {
  const { t } = useTranslation();
  const [rules, setRules] = useState<LearnedRule[] | null>(null);

  const load = useCallback(() => {
    yuyinApi
      .learned()
      .then(setRules)
      .catch((e) => console.error("Failed to load learned corrections:", e));
  }, []);
  useEffect(() => {
    load();
    window.addEventListener("focus", load);
    return () => window.removeEventListener("focus", load);
  }, [load]);

  const set = async (rule: LearnedRule, active: boolean) => {
    await yuyinApi.setLearned(rule.from, rule.to, active);
    load();
  };

  return (
    <Card className="p-4 flex flex-col gap-3">
      <div className="flex justify-between items-baseline gap-3">
        <span className="text-[12px] font-semibold text-text">
          {t("moqi.learn.learnedTitle")}
        </span>
        <span className="text-[11px] text-muted">
          {t("moqi.learn.learnedHint")}
        </span>
      </div>
      {rules && rules.length === 0 ? (
        <p className="m-0 text-[12px] leading-relaxed text-muted">
          {enabled ? t("moqi.learn.emptyOn") : t("moqi.learn.emptyOff")}
        </p>
      ) : (
        <ul className="m-0 p-0 list-none flex flex-col divide-y divide-hairline">
          {rules?.map((rule) => (
            <li
              key={`${rule.from}→${rule.to}`}
              className="flex items-center justify-between gap-3 py-2"
            >
              <span className="min-w-0 flex flex-col gap-0.5">
                <span className="text-[13px] text-text select-text">
                  <span className="text-muted line-through decoration-muted/60">
                    {rule.from}
                  </span>
                  {" → "}
                  <span className="font-medium">{rule.to}</span>
                </span>
                <span
                  className={`text-[11px] ${rule.active ? "text-positive" : "text-muted"}`}
                >
                  {rule.active
                    ? t("moqi.learn.applied")
                    : t("moqi.learn.seenOnce")}
                </span>
              </span>
              <span className="flex gap-1.5 shrink-0">
                {!rule.active && (
                  <SmallButton onClick={() => set(rule, true)}>
                    {t("moqi.learn.applyNow")}
                  </SmallButton>
                )}
                <SmallButton onClick={() => set(rule, false)}>
                  {t("moqi.learn.remove")}
                </SmallButton>
              </span>
            </li>
          ))}
        </ul>
      )}
    </Card>
  );
};

export const DictionaryPage: React.FC = () => {
  const { t } = useTranslation();
  const [config, setConfig] = useState<YuyinConfig | null>(null);
  const [draft, setDraft] = useState("");

  useEffect(() => {
    yuyinApi
      .getConfig()
      .then(setConfig)
      .catch((e) => console.error("Failed to load dictionary:", e));
  }, []);

  // Starts from the saved list: learning may have added a word meanwhile.
  const change = async (edit: (vocab: string[]) => string[]) => {
    try {
      const latest = await yuyinApi.getConfig();
      setConfig(await yuyinApi.updateConfig({ vocab: edit(latest.vocab) }));
    } catch (e) {
      console.error("Failed to save dictionary:", e);
      toast.error(t("moqi.dictionary.saveFailed"));
    }
  };

  const add = () => {
    const word = draft.trim();
    if (!config || !word) return;
    change((vocab) => (vocab.includes(word) ? vocab : [...vocab, word]));
    setDraft("");
  };

  return (
    <div className="flex flex-col gap-5">
      <PageTitle
        title={t("moqi.nav.dictionary")}
        subtitle={t("moqi.dictionary.subtitle")}
      />

      <form
        className="flex gap-2"
        onSubmit={(e) => {
          e.preventDefault();
          add();
        }}
      >
        <TextInput
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          placeholder={t("moqi.dictionary.placeholder")}
          aria-label={t("moqi.dictionary.placeholder")}
          className="flex-1"
        />
        <button
          type="submit"
          disabled={!draft.trim()}
          className="text-[13px] font-medium px-[18px] rounded-[9px] bg-logo-primary text-white disabled:opacity-40 hover:brightness-110 transition"
        >
          {t("moqi.dictionary.add")}
        </button>
      </form>

      <Card className="p-4 flex flex-col gap-3">
        <div className="flex justify-between items-baseline">
          <span className="text-[12px] font-semibold text-text">
            {t("moqi.dictionary.count", { count: config?.vocab.length ?? 0 })}
          </span>
          <span className="text-[11px] text-muted">
            {t("moqi.dictionary.removeHint")}
          </span>
        </div>
        <div className="flex flex-wrap gap-1.5">
          {config?.vocab.map((word) => (
            <span
              key={word}
              className="inline-flex items-center gap-0.5 text-[12.5px] ps-2.5 pe-1 py-1 rounded-[7px] bg-fill text-text select-text"
            >
              {word}
              <button
                type="button"
                onClick={() =>
                  change((vocab) => vocab.filter((w) => w !== word))
                }
                aria-label={t("moqi.dictionary.remove", { word })}
                className="w-[18px] h-[18px] rounded-[5px] text-muted hover:text-text hover:bg-black/5 dark:hover:bg-white/10 leading-none"
              >
                ×
              </button>
            </span>
          ))}
        </div>
      </Card>

      <SnippetsCard
        snippets={config?.snippets ?? []}
        change={async (edit) => {
          try {
            const latest = await yuyinApi.getConfig();
            setConfig(
              await yuyinApi.updateConfig({ snippets: edit(latest.snippets) }),
            );
          } catch (e) {
            console.error("Failed to save snippets:", e);
            toast.error(t("moqi.dictionary.saveFailed"));
          }
        }}
      />

      <LearnedList enabled={config?.learn_from_edits === true} />
    </div>
  );
};
