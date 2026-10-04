// Yuyin fork: every setting Moqi has — shortcut, clean-up level, clean-up
// service and key, microphone, general — plus About. Handy's other settings
// keep the defaults written by yuyin/defaults.rs.
import React, {
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
} from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { ShortcutInput } from "@/components/settings/ShortcutInput";
import { MicrophoneSelector } from "@/components/settings/MicrophoneSelector";
import { useSettings } from "@/hooks/useSettings";
import { useOsType } from "@/hooks/useOsType";
import { getSupportedLanguage } from "@/i18n";
import { applyTheme, THEME_OPTIONS } from "@/lib/utils/theme";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { checkForUpdate, installUpdate, useUpdateState } from "../update";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  yuyinApi,
  type AppStyle,
  type Level,
  type ModelInfo,
  type RecentApp,
  type WritingContext,
  type Service,
  type SyncStatus,
  type YuyinConfig,
} from "../api";
import {
  OPENROUTER,
  OPENROUTER_PRESETS,
  costPerThousand,
  formatCost,
} from "../openrouter";
import { YuyinAbout } from "../YuyinAbout";
import {
  Group,
  Kbd,
  Row,
  Segmented,
  Select,
  SmallButton,
  Switch,
  TextInput,
} from "./ui";
import { BackIcon, ChevronIcon } from "./icons";
import { tapKeyLabel } from "./format";
import { serviceName } from "./HomePage";

/** "https://api.example.com/v1" → "api.example.com"; a bare host as it is. */
const hostOf = (url: string) => {
  try {
    return new URL(url.includes("://") ? url : `https://${url}`).host;
  } catch {
    return url;
  }
};

const DEEPSEEK = {
  base_url: "https://api.deepseek.com",
  model: "deepseek-flash",
};

/** Ollama's address; LM Studio users change the port to 1234. */
const LOCAL = {
  base_url: "http://localhost:11434/v1",
  model: "",
};

/** Where to get a key, for the services we know. */
const KEY_PAGES: Partial<Record<Service, { name: string; url: string }>> = {
  deepseek: { name: "DeepSeek", url: "https://platform.deepseek.com/api_keys" },
  openrouter: { name: "OpenRouter", url: "https://openrouter.ai/keys" },
};

/** The key for the service at `baseUrl`: each service has its own, so one
 * provider's key is never sent to another. */
const ApiKeyRow: React.FC<{
  baseUrl: string;
  service: Service;
  /** Whether a key is stored, once known and after every change. */
  onKeyKnown?: (hasKey: boolean) => void;
}> = ({ baseUrl, service, onKeyKnown }) => {
  const { t } = useTranslation();
  const keyPage = KEY_PAGES[service];
  const [hasKey, setHasKey] = useState<boolean | null>(null);
  const [editing, setEditing] = useState(false);
  const [key, setKey] = useState("");
  const [testing, setTesting] = useState(false);
  const [result, setResult] = useState<{
    ok: boolean;
    text: string;
    seconds?: string;
  } | null>(null);

  const hasAddress = baseUrl.trim().length > 0;

  useEffect(() => {
    setResult(null);
    setEditing(false);
    if (!hasAddress) {
      setHasKey(false);
      return;
    }
    yuyinApi
      .hasApiKey(baseUrl)
      .then(setHasKey)
      .catch(() => setHasKey(false));
  }, [baseUrl, hasAddress]);

  useEffect(() => {
    if (hasKey !== null) onKeyKnown?.(hasKey);
  }, [hasKey, onKeyKnown]);

  const save = async () => {
    try {
      await yuyinApi.setApiKey(key.trim(), baseUrl);
      setHasKey(key.trim().length > 0);
      setKey("");
      setEditing(false);
      toast.success(t("moqi.settings.keySaved"));
    } catch (e) {
      toast.error(String(e));
    }
  };

  const test = async () => {
    setTesting(true);
    setResult(null);
    const started = performance.now();
    try {
      const text = await yuyinApi.testPolish(t("yuyin.test.sample"));
      const seconds = ((performance.now() - started) / 1000).toFixed(1);
      setResult({ ok: true, text, seconds });
    } catch (e) {
      setResult({ ok: false, text: String(e) });
    } finally {
      setTesting(false);
    }
  };

  return (
    <>
      <Row label={t("moqi.settings.apiKey")}>
        {editing ? (
          <>
            <TextInput
              type="password"
              value={key}
              autoFocus
              onChange={(e) => setKey(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && save()}
              placeholder={service === "openrouter" ? "sk-or-…" : "sk-…"}
              aria-label={t("moqi.settings.apiKey")}
              className="w-[220px] !py-1 !text-[12px]"
            />
            <SmallButton onClick={save} disabled={!key.trim()}>
              {t("moqi.settings.save")}
            </SmallButton>
            <SmallButton onClick={() => setEditing(false)}>
              {t("moqi.settings.cancel")}
            </SmallButton>
          </>
        ) : (
          <>
            <span
              className={`text-[12px] ${hasKey ? "text-positive" : "text-muted"}`}
            >
              {hasKey === null
                ? ""
                : hasKey
                  ? t("moqi.settings.keyStored")
                  : t("moqi.settings.keyMissing")}
            </span>
            <SmallButton
              onClick={() => setEditing(true)}
              disabled={!hasAddress}
            >
              {hasKey
                ? t("moqi.settings.replace")
                : t("moqi.settings.enterKey")}
            </SmallButton>
            <SmallButton onClick={test} disabled={!hasKey || testing}>
              {testing ? t("moqi.settings.testing") : t("moqi.settings.test")}
            </SmallButton>
          </>
        )}
      </Row>
      {result && (
        <div
          className={`px-3.5 py-2.5 text-[12px] leading-relaxed select-text ${
            result.ok ? "text-text" : "text-error"
          }`}
        >
          {result.ok
            ? t("moqi.settings.testResultTimed", {
                text: result.text,
                seconds: result.seconds,
              })
            : result.text}
        </div>
      )}
      {hasKey === false && keyPage && (
        <div className="px-3.5 py-2">
          <button
            type="button"
            onClick={() => openUrl(keyPage.url)}
            className="text-[12px] text-logo-primary hover:underline"
          >
            {t("moqi.settings.getKey", { service: keyPage.name })}
          </button>
        </div>
      )}
    </>
  );
};

/** Sync through a folder in the user's cloud drive (sync.rs). */
const SyncGroup: React.FC = () => {
  const { t, i18n } = useTranslation();
  const [status, setStatus] = useState<SyncStatus | null>(null);
  const [busy, setBusy] = useState(false);

  const load = () =>
    yuyinApi
      .syncStatus()
      .then(setStatus)
      .catch((e) => console.error("Failed to read sync status:", e));
  useEffect(() => {
    load();
  }, []);

  const when = (ms: number) => {
    const d = new Date(ms);
    const today = d.toDateString() === new Date().toDateString();
    return d.toLocaleString(i18n.language, {
      ...(today ? {} : { month: "numeric", day: "numeric" }),
      hour: "2-digit",
      minute: "2-digit",
    });
  };

  const choose = async () => {
    const folder = await openDialog({
      directory: true,
      title: t("moqi.sync.chooseTitle"),
    });
    if (typeof folder !== "string") return;
    try {
      await yuyinApi.setSyncFolder(folder);
      await syncNow();
    } catch (e) {
      toast.error(String(e));
    }
  };
  const stop = async () => {
    await yuyinApi.setSyncFolder(null);
    load();
  };
  const syncNow = async () => {
    setBusy(true);
    try {
      await yuyinApi.syncNow();
    } catch (e) {
      console.error("Sync failed:", e);
    } finally {
      setBusy(false);
      load();
    }
  };

  const folder = status?.folder;
  const devices = status?.devices ?? [];
  return (
    <Group title={t("moqi.sync.groupTitle")} footnote={t("moqi.sync.footnote")}>
      <Row
        label={t("moqi.sync.folder")}
        description={
          <span className="break-all">{folder ?? t("moqi.sync.notSet")}</span>
        }
      >
        {folder && (
          <SmallButton onClick={stop}>{t("moqi.sync.stop")}</SmallButton>
        )}
        <SmallButton onClick={choose}>{t("moqi.sync.choose")}</SmallButton>
      </Row>
      {folder && (
        <Row
          label={t("moqi.sync.status")}
          description={
            status?.error ? (
              <span className="text-error">
                {t("moqi.sync.failed", { error: status.error })}
              </span>
            ) : (
              [
                status && status.last_sync > 0
                  ? t("moqi.sync.lastSync", { time: when(status.last_sync) })
                  : t("moqi.sync.never"),
                devices.length > 0
                  ? t("moqi.sync.devices", {
                      count: devices.length,
                      names: devices.map(([name]) => name).join("、"),
                    })
                  : t("moqi.sync.noDevices"),
              ].join("・")
            )
          }
        >
          <SmallButton onClick={syncNow} disabled={busy}>
            {busy ? t("moqi.sync.syncing") : t("moqi.sync.syncNow")}
          </SmallButton>
        </Row>
      )}
    </Group>
  );
};

/** A model server on this computer: its address and the model to use,
 * picked from what the server offers. */
const LocalModelRow: React.FC<{ config: YuyinConfig; save: Save }> = ({
  config,
  save,
}) => {
  const { t } = useTranslation();
  const [baseUrl, setBaseUrl] = useState(config.base_url);
  const [models, setModels] = useState<string[] | null>(null);
  const [failed, setFailed] = useState(false);

  const load = useCallback(() => {
    setFailed(false);
    setModels(null);
    yuyinApi
      .localModels(config.base_url)
      .then(setModels)
      .catch(() => setFailed(true));
  }, [config.base_url]);
  useEffect(() => {
    load();
  }, [load]);

  // The first model the server offers, once there is one and none is chosen.
  useEffect(() => {
    if (models && models.length > 0 && !config.model)
      save({ model: models[0] });
  }, [models, config.model, save]);

  return (
    <>
      <Row label={t("moqi.settings.endpoint")} htmlFor="moqi-base-url">
        <TextInput
          id="moqi-base-url"
          value={baseUrl}
          onChange={(e) => setBaseUrl(e.target.value)}
          onBlur={() =>
            baseUrl.trim() !== config.base_url &&
            save({ base_url: baseUrl.trim(), model: "" })
          }
          placeholder={LOCAL.base_url}
          className="w-[220px] !py-1 !text-[12px]"
        />
        <SmallButton onClick={load}>
          {t("moqi.settings.localRefresh")}
        </SmallButton>
      </Row>
      <Row
        label={t("moqi.settings.model")}
        htmlFor="moqi-local-model"
        description={
          failed
            ? t("moqi.settings.localNotFound")
            : models && models.length === 0
              ? t("moqi.settings.localNoModels")
              : undefined
        }
      >
        {failed || (models && models.length === 0) ? (
          <SmallButton onClick={() => openUrl("https://ollama.com/download")}>
            {t("moqi.settings.localGetOllama")}
          </SmallButton>
        ) : (
          <Select
            id="moqi-local-model"
            value={config.model}
            disabled={!models}
            onChange={(e) => save({ model: e.target.value })}
          >
            {(models ?? (config.model ? [config.model] : [])).map((m) => (
              <option key={m} value={m}>
                {m}
              </option>
            ))}
          </Select>
        )}
      </Row>
    </>
  );
};

/** Output languages the clean-up can write in (prompt.rs language_name). */
const LANGUAGES = ["en", "ja", "ko", "zh-Hans"];
const STYLES: WritingContext[] = ["chat", "to_ai", "notes", "other"];

/** One app's style, extra instruction and language. */
const AppStyleEditor: React.FC<{
  recent: RecentApp;
  style: AppStyle | undefined;
  onChange: (style: AppStyle | null) => void;
}> = ({ recent, style, onChange }) => {
  const { t } = useTranslation();
  const current: AppStyle = style ?? {
    app: recent.app,
    name: recent.name,
    context: null,
    note: "",
    translate_to: null,
  };
  const [note, setNote] = useState(current.note);
  const set = (patch: Partial<AppStyle>) => {
    const next = { ...current, ...patch };
    const plain =
      next.context === null && !next.note.trim() && next.translate_to === null;
    onChange(plain ? null : next);
  };
  return (
    <div className="px-3.5 pb-3 pt-1 grid grid-cols-[auto_minmax(0,1fr)] gap-x-3 gap-y-2 items-center">
      <span className="text-[12px] text-muted">{t("moqi.styles.style")}</span>
      <Select
        value={current.context ?? ""}
        onChange={(e) =>
          set({ context: (e.target.value || null) as WritingContext | null })
        }
        className="justify-self-start"
      >
        <option value="">
          {t("moqi.styles.auto", { style: t(`moqi.styles.${recent.context}`) })}
        </option>
        {STYLES.map((s) => (
          <option key={s} value={s}>
            {t(`moqi.styles.${s}`)}
          </option>
        ))}
      </Select>
      <span className="text-[12px] text-muted">
        {t("moqi.styles.language")}
      </span>
      <Select
        value={current.translate_to ?? "__global"}
        onChange={(e) =>
          set({
            translate_to: e.target.value === "__global" ? null : e.target.value,
          })
        }
        className="justify-self-start"
      >
        <option value="__global">{t("moqi.styles.followGlobal")}</option>
        <option value="">{t("moqi.styles.asSpoken")}</option>
        {LANGUAGES.map((l) => (
          <option key={l} value={l}>
            {t(`moqi.languages.${l}`)}
          </option>
        ))}
      </Select>
      <span className="text-[12px] text-muted">{t("moqi.styles.note")}</span>
      <TextInput
        value={note}
        onChange={(e) => setNote(e.target.value)}
        onBlur={() => note !== current.note && set({ note })}
        placeholder={t("moqi.styles.notePlaceholder")}
        className="!py-1 !text-[12px]"
      />
    </div>
  );
};

/** The output language, and per-app styles for the apps used recently. */
const StylesGroup: React.FC<{ config: YuyinConfig; save: Save }> = ({
  config,
  save,
}) => {
  const { t } = useTranslation();
  const [recent, setRecent] = useState<RecentApp[] | null>(null);
  const [open, setOpen] = useState<string | null>(null);
  useEffect(() => {
    yuyinApi
      .recentApps()
      .then(setRecent)
      .catch((e) => console.error("Failed to load recent apps:", e));
  }, []);

  const styleOf = (app: string) => config.app_styles.find((s) => s.app === app);
  const setStyle = (app: string, style: AppStyle | null) =>
    save({
      app_styles: [
        ...config.app_styles.filter((s) => s.app !== app),
        ...(style ? [style] : []),
      ],
    });
  const summary = (r: RecentApp) => {
    const s = styleOf(r.app);
    if (!s)
      return t("moqi.styles.auto", { style: t(`moqi.styles.${r.context}`) });
    return [
      s.context
        ? t(`moqi.styles.${s.context}`)
        : t("moqi.styles.auto", { style: t(`moqi.styles.${r.context}`) }),
      s.translate_to === ""
        ? t("moqi.styles.asSpoken")
        : s.translate_to
          ? t("moqi.styles.translatesTo", {
              language: t(`moqi.languages.${s.translate_to}`),
            })
          : "",
      s.note.trim(),
    ]
      .filter(Boolean)
      .join("・");
  };

  return (
    <Group
      title={t("moqi.styles.groupTitle")}
      footnote={t("moqi.styles.footnote")}
    >
      <Row label={t("moqi.styles.translateAll")} htmlFor="moqi-translate">
        <Select
          id="moqi-translate"
          value={config.translate_to ?? ""}
          onChange={(e) => save({ translate_to: e.target.value || null })}
        >
          <option value="">{t("moqi.styles.asSpoken")}</option>
          {LANGUAGES.map((l) => (
            <option key={l} value={l}>
              {t(`moqi.languages.${l}`)}
            </option>
          ))}
        </Select>
      </Row>
      {recent && recent.length === 0 && (
        <p className="m-0 px-3.5 py-2.5 text-[12px] text-muted">
          {t("moqi.styles.empty")}
        </p>
      )}
      {recent?.map((r) => (
        <div key={r.app}>
          <Row label={r.name || r.app} description={summary(r)}>
            <SmallButton onClick={() => setOpen(open === r.app ? null : r.app)}>
              {open === r.app ? t("moqi.styles.done") : t("moqi.styles.adjust")}
            </SmallButton>
          </Row>
          {open === r.app && (
            <AppStyleEditor
              recent={r}
              style={styleOf(r.app)}
              onChange={(s) => setStyle(r.app, s)}
            />
          )}
        </div>
      ))}
    </Group>
  );
};

/** Check for updates automatically, or now. */
const UpdateRow: React.FC<{
  enabled: boolean;
  setEnabled: (v: boolean) => void;
}> = ({ enabled, setEnabled }) => {
  const { t } = useTranslation();
  const update = useUpdateState();
  const status =
    update.kind === "checking"
      ? t("moqi.update.checking")
      : update.kind === "latest"
        ? t("moqi.update.latest")
        : update.kind === "available"
          ? t("moqi.update.availableShort", { version: update.version })
          : update.kind === "installing"
            ? t("moqi.update.installing", { percent: update.percent })
            : update.kind === "failed"
              ? t("moqi.update.failed")
              : update.kind === "installFailed"
                ? t("moqi.update.installFailed")
                : t("moqi.update.description");
  return (
    <Row label={t("moqi.update.title")} description={status}>
      {update.kind === "available" || update.kind === "installFailed" ? (
        <SmallButton onClick={installUpdate}>
          {t("moqi.update.install")}
        </SmallButton>
      ) : (
        <SmallButton
          onClick={() => checkForUpdate(true)}
          disabled={update.kind === "checking" || update.kind === "installing"}
        >
          {t("moqi.update.checkNow")}
        </SmallButton>
      )}
      <Switch
        label={t("moqi.update.title")}
        checked={enabled}
        onChange={setEnabled}
      />
    </Row>
  );
};

const OTHER = "__other";

/** OpenRouter's model: one we recommend, or any from its catalog. */
const ModelPicker: React.FC<{
  config: YuyinConfig;
  save: Save;
}> = ({ config, save }) => {
  const { t } = useTranslation();
  const preset = OPENROUTER_PRESETS.find((p) => p.id === config.model);
  const [browsing, setBrowsing] = useState(false);
  const [models, setModels] = useState<ModelInfo[] | null>(null);
  const [failed, setFailed] = useState(false);
  const [query, setQuery] = useState("");

  // The catalog (and prices) load once the picker is on screen.
  useEffect(() => {
    if (models) return;
    setFailed(false);
    yuyinApi
      .openrouterModels()
      .then(setModels)
      .catch(() => setFailed(true));
  }, [models]);

  const showCatalog = browsing || !preset;
  const choose = (id: string) => {
    setBrowsing(false);
    setQuery("");
    if (id !== config.model) save({ model: id });
  };
  const cost = (m?: ModelInfo) =>
    !m
      ? ""
      : m.input_price === 0 && m.output_price === 0
        ? t("moqi.settings.modelFree")
        : t("moqi.settings.costPerThousand", {
            cost: formatCost(costPerThousand(m)),
          });
  const priced = (id: string) => models?.find((m) => m.id === id);

  const q = query.trim().toLowerCase();
  const matches = (models ?? [])
    .filter(
      (m) =>
        !q ||
        m.id.toLowerCase().includes(q) ||
        m.name.toLowerCase().includes(q),
    )
    .slice(0, 60);

  return (
    <>
      <Row
        label={t("moqi.settings.model")}
        htmlFor="moqi-model"
        description={
          preset && !browsing
            ? [
                t(`moqi.settings.openrouterNotes.${preset.note}`),
                cost(priced(preset.id)),
              ]
                .filter(Boolean)
                .join("・")
            : config.model
        }
      >
        <Select
          id="moqi-model"
          value={showCatalog ? OTHER : config.model}
          onChange={(e) =>
            e.target.value === OTHER
              ? setBrowsing(true)
              : choose(e.target.value)
          }
        >
          {OPENROUTER_PRESETS.map((p) => (
            <option key={p.id} value={p.id}>
              {p.name}
            </option>
          ))}
          <option value={OTHER}>{t("moqi.settings.otherModel")}</option>
        </Select>
      </Row>
      {showCatalog && (
        <div className="px-3.5 py-2.5 flex flex-col gap-2">
          <TextInput
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder={t("moqi.settings.searchModels")}
            aria-label={t("moqi.settings.searchModels")}
            className="!py-1 !text-[12px]"
          />
          {failed ? (
            <p className="m-0 text-[12px] text-error">
              {t("moqi.settings.modelsFailed")}
            </p>
          ) : !models ? (
            <p className="m-0 text-[12px] text-muted">
              {t("moqi.settings.modelsLoading")}
            </p>
          ) : (
            <ul className="m-0 p-0 list-none max-h-[220px] overflow-y-auto flex flex-col">
              {matches.map((m) => (
                <li key={m.id}>
                  <button
                    type="button"
                    onClick={() => choose(m.id)}
                    className={`w-full text-start px-2 py-1.5 rounded-[7px] hover:bg-fill flex justify-between items-baseline gap-3 ${
                      m.id === config.model ? "bg-fill" : ""
                    }`}
                  >
                    <span className="min-w-0 flex flex-col">
                      <span className="text-[12px] text-text truncate">
                        {m.name}
                      </span>
                      <span className="text-[11px] text-muted truncate">
                        {m.id}
                      </span>
                    </span>
                    <span className="text-[11px] text-muted whitespace-nowrap">
                      {cost(m)}
                    </span>
                  </button>
                </li>
              ))}
            </ul>
          )}
        </div>
      )}
    </>
  );
};

type Save = (patch: Partial<YuyinConfig>) => void;

const CustomServiceRow: React.FC<{
  config: YuyinConfig;
  save: Save;
}> = ({ config, save }) => {
  const { t } = useTranslation();
  const [baseUrl, setBaseUrl] = useState(config.base_url);
  const [model, setModel] = useState(config.model);
  const commit = () => {
    if (baseUrl !== config.base_url || model !== config.model) {
      save({ base_url: baseUrl.trim(), model: model.trim() });
    }
  };
  return (
    <Row label={t("moqi.settings.endpoint")} htmlFor="moqi-base-url">
      <TextInput
        id="moqi-base-url"
        value={baseUrl}
        onChange={(e) => setBaseUrl(e.target.value)}
        onBlur={commit}
        placeholder="https://…/v1"
        className="w-[190px] !py-1 !text-[12px]"
      />
      <TextInput
        value={model}
        onChange={(e) => setModel(e.target.value)}
        onBlur={commit}
        placeholder={t("moqi.settings.modelName")}
        aria-label={t("moqi.settings.modelName")}
        className="w-[120px] !py-1 !text-[12px]"
      />
    </Row>
  );
};

export const SettingsPage: React.FC = () => {
  const { t, i18n } = useTranslation();
  const os = useOsType();
  const { settings, updateSetting } = useSettings();
  const [config, setConfig] = useState<YuyinConfig | null>(null);
  const [about, setAbout] = useState(false);
  const [keyReady, setKeyReady] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);

  // About is a sub-view, not a page: the window only resets the scroll on a
  // page change, so start About (and the way back) at the top here.
  useLayoutEffect(() => {
    rootRef.current?.closest(".overflow-y-auto")?.scrollTo({ top: 0 });
  }, [about]);

  useEffect(() => {
    yuyinApi
      .getConfig()
      .then(setConfig)
      .catch((e) => console.error(e));
  }, []);

  const save: Save = async (patch) => {
    setConfig((c) => (c ? { ...c, ...patch } : c));
    try {
      setConfig(await yuyinApi.updateConfig(patch));
    } catch (e) {
      toast.error(String(e));
    }
  };

  if (about) {
    return (
      <div ref={rootRef} className="flex flex-col gap-4">
        <button
          type="button"
          onClick={() => setAbout(false)}
          className="self-start inline-flex items-center gap-1 text-[13px] text-logo-primary"
        >
          <BackIcon size={13} />
          {t("moqi.nav.settings")}
        </button>
        <YuyinAbout />
      </div>
    );
  }

  const service: Service = config?.service ?? "deepseek";
  const localOnly = service === "none";
  const level: Level = localOnly ? "raw" : (config?.level ?? "tidy");
  // A saved key (or a chosen local model) does nothing at Raw. Users who set
  // up DeepSeek expect it to work, so say so where the level is chosen.
  const serviceReady =
    service === "local" ? Boolean(config?.model) : !localOnly && keyReady;
  const readyName =
    service === "deepseek"
      ? "DeepSeek"
      : service === "openrouter"
        ? "OpenRouter"
        : serviceName(hostOf(config?.base_url ?? ""));
  const language =
    getSupportedLanguage(settings?.app_language) || i18n.language;
  const tapKey = tapKeyLabel(
    settings?.bindings?.transcribe?.current_binding,
    os,
    t,
  );

  return (
    <div ref={rootRef} className="flex flex-col gap-[18px] max-w-[600px]">
      <h1 className="m-0 mb-1 text-[26px] font-bold tracking-[-0.02em] text-text">
        {t("moqi.nav.settings")}
      </h1>

      <Group title={t("moqi.settings.shortcuts")}>
        <ShortcutInput
          shortcutId="transcribe"
          grouped
          descriptionMode="inline"
        />
        <Row
          label={t("moqi.settings.handsFree")}
          description={t("moqi.settings.handsFreeHint")}
        >
          <span className="flex gap-[3px]">
            <Kbd>{tapKey}</Kbd>
            <Kbd>{tapKey}</Kbd>
          </span>
        </Row>
        <Row label={t("moqi.settings.cancelRecording")}>
          <Kbd>{t("moqi.keys.esc")}</Kbd>
        </Row>
      </Group>

      <Group title={t("moqi.settings.level")}>
        <div className="px-3.5 py-3 flex flex-col gap-2.5">
          <Segmented
            label={t("moqi.settings.level")}
            value={level}
            disabled={!config || localOnly}
            onChange={(v) => config && save({ level: v })}
            options={(["raw", "tidy", "polish"] as Level[]).map((v) => ({
              value: v,
              label: t(`moqi.levels.${v}`),
            }))}
          />
          <p className="m-0 text-[12px] leading-relaxed text-text/80">
            {t(`moqi.settings.levelNote.${level}`)}
          </p>
          {config && level === "raw" && serviceReady && (
            <div className="flex items-center gap-3 rounded-lg bg-logo-primary/10 px-3 py-2">
              <p className="m-0 flex-1 text-[12px] leading-relaxed text-text">
                {service === "local"
                  ? t("moqi.settings.rawSkipsLocal")
                  : t("moqi.settings.rawSkips", { service: readyName })}
              </p>
              <SmallButton onClick={() => save({ level: "tidy" })}>
                {t("moqi.settings.useTidy")}
              </SmallButton>
            </div>
          )}
        </div>
        <Row
          label={t("moqi.editSelection.title")}
          description={t("moqi.editSelection.description")}
        >
          <Switch
            label={t("moqi.editSelection.title")}
            checked={config?.edit_selection === true}
            disabled={!config}
            onChange={(v) => save({ edit_selection: v })}
          />
        </Row>
      </Group>

      <Group
        title={t("moqi.settings.service")}
        footnote={
          localOnly
            ? t("moqi.settings.serviceNoteLocal")
            : service === "openrouter"
              ? t("moqi.settings.serviceNoteOpenrouter")
              : service === "local"
                ? t("moqi.settings.serviceNoteLocalModel")
                : t("moqi.settings.serviceNote")
        }
      >
        <Row label={t("moqi.settings.serviceLabel")} htmlFor="moqi-service">
          <Select
            id="moqi-service"
            value={service}
            disabled={!config}
            onChange={(e) => {
              if (!config) return;
              const next = e.target.value as Service;
              save(
                next === "deepseek"
                  ? { service: next, ...DEEPSEEK }
                  : next === "openrouter"
                    ? { service: next, ...OPENROUTER }
                    : next === "local"
                      ? { service: next, ...LOCAL }
                      : { service: next },
              );
            }}
          >
            <option value="deepseek">
              {t("moqi.settings.serviceDeepseek")}
            </option>
            <option value="openrouter">
              {t("moqi.settings.serviceOpenrouter")}
            </option>
            <option value="local">{t("moqi.settings.serviceLocal")}</option>
            <option value="custom">{t("moqi.settings.serviceCustom")}</option>
            <option value="none">{t("moqi.settings.serviceNone")}</option>
          </Select>
        </Row>
        {!localOnly && service !== "local" && (
          <ApiKeyRow
            service={service}
            onKeyKnown={setKeyReady}
            baseUrl={
              service === "deepseek"
                ? DEEPSEEK.base_url
                : service === "openrouter"
                  ? OPENROUTER.base_url
                  : (config?.base_url ?? "")
            }
          />
        )}
        {config && service === "openrouter" && (
          <ModelPicker config={config} save={save} />
        )}
        {config && service === "local" && (
          <LocalModelRow config={config} save={save} />
        )}
        {config && service === "custom" && (
          <CustomServiceRow config={config} save={save} />
        )}
      </Group>

      {config && <StylesGroup config={config} save={save} />}

      <Group
        title={t("moqi.learn.settingsGroup")}
        footnote={t("moqi.learn.privacy")}
      >
        <Row
          label={t("moqi.learn.title")}
          description={t("moqi.learn.description")}
        >
          <Switch
            label={t("moqi.learn.title")}
            checked={config?.learn_from_edits === true}
            disabled={!config}
            onChange={(v) => save({ learn_from_edits: v })}
          />
        </Row>
      </Group>

      <SyncGroup />

      <Group title={t("moqi.settings.microphone")}>
        <MicrophoneSelector grouped descriptionMode="inline" />
        <Row
          label={t("moqi.settings.sound")}
          description={t("moqi.settings.soundHint")}
        >
          <Switch
            label={t("moqi.settings.sound")}
            checked={settings?.audio_feedback ?? true}
            onChange={(v) => updateSetting("audio_feedback", v)}
          />
        </Row>
      </Group>

      <Group title={t("moqi.settings.general")}>
        <Row label={t("moqi.settings.autostart")}>
          <Switch
            label={t("moqi.settings.autostart")}
            checked={settings?.autostart_enabled ?? false}
            onChange={(v) => updateSetting("autostart_enabled", v)}
          />
        </Row>
        <UpdateRow
          enabled={settings?.update_checks_enabled ?? false}
          setEnabled={(v) => updateSetting("update_checks_enabled", v)}
        />
        <Row label={t("moqi.settings.language")} htmlFor="moqi-language">
          <Select
            id="moqi-language"
            value={language}
            onChange={(e) => {
              i18n.changeLanguage(e.target.value);
              updateSetting("app_language", e.target.value);
            }}
          >
            {/* Language names are shown in their own language. */}
            {/* eslint-disable-next-line i18next/no-literal-string */}
            <option value="zh-TW">繁體中文</option>
            {/* eslint-disable-next-line i18next/no-literal-string */}
            <option value="en">English</option>
          </Select>
        </Row>
        <Row label={t("moqi.settings.appearance")} htmlFor="moqi-theme">
          <Select
            id="moqi-theme"
            value={settings?.theme ?? "system"}
            onChange={(e) => {
              const theme = e.target.value as (typeof THEME_OPTIONS)[number];
              applyTheme(theme);
              updateSetting("theme", theme);
            }}
          >
            {THEME_OPTIONS.map((v) => (
              <option key={v} value={v}>
                {t(`theme.options.${v}`)}
              </option>
            ))}
          </Select>
        </Row>
        <button
          type="button"
          onClick={() => setAbout(true)}
          className="w-full flex items-center justify-between min-h-11 px-3.5 text-[13px] text-text text-start"
        >
          {t("moqi.settings.about")}
          <ChevronIcon size={12} className="text-muted" />
        </button>
      </Group>
    </div>
  );
};
