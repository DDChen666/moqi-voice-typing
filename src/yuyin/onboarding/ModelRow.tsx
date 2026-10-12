// Yuyin fork: the speech model as the third setup item. There is no picker:
// the backend says which model this computer can dictate with (M0's pick,
// or on a Windows PC without a usable GPU a light one, yuyin/hardware.rs),
// and the download starts as soon as the welcome page opens, while the user
// grants permissions.
import React, { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { useModelStore } from "@/stores/modelStore";
import { useSettings } from "@/hooks/useSettings";
import { yuyinApi } from "../api";
import { CheckIcon, DownloadIcon } from "../window/icons";
import { SmallButton } from "../window/ui";

/** Qwen3-ASR 1.7B, Q5_K_M: the engine M0 chose (see yuyin/defaults.rs). */
export const MOQI_MODEL_ID =
  "handy-computer/Qwen3-ASR-1.7B-gguf/Qwen3-ASR-1.7B-Q5_K_M.gguf";

export interface MoqiModelState {
  ready: boolean;
  busy: boolean;
  failed: boolean;
  /** 0–100 while downloading. */
  percent: number | null;
  downloaded: number;
  total: number;
  retry: () => void;
}

/** Downloads (once) and selects the model; reports progress. */
export function useMoqiModel(preview: boolean): MoqiModelState {
  const {
    models,
    downloadModel,
    selectModel,
    downloadingModels,
    verifyingModels,
    extractingModels,
    downloadProgress,
  } = useModelStore();
  const { settings } = useSettings();
  const [failed, setFailed] = useState(false);
  const [attempt, setAttempt] = useState(0);
  const started = useRef(-1);
  const selected = useRef(false);
  // Which model, asked once; nothing downloads until the answer is in.
  const [modelId, setModelId] = useState<string | null>(null);
  useEffect(() => {
    yuyinApi
      .recommendedModel()
      .then(setModelId)
      .catch(() => setModelId(MOQI_MODEL_ID));
  }, []);
  // A model missing from the list (a catalog change) never strands the
  // user: they get M0's pick instead.
  const listed = models.length === 0 || models.some((m) => m.id === modelId);
  const id = modelId && listed ? modelId : MOQI_MODEL_ID;

  const model = modelId ? models.find((m) => m.id === id) : undefined;
  const ready = !!model?.is_downloaded;
  const busy =
    id in downloadingModels || id in verifyingModels || id in extractingModels;

  useEffect(() => {
    if (preview || !model || ready || busy || started.current === attempt)
      return;
    started.current = attempt;
    setFailed(false);
    downloadModel(model.id).then((ok) => {
      if (!ok) setFailed(true);
    });
  }, [preview, model, ready, busy, attempt, downloadModel]);

  // Selecting it loads it and marks onboarding complete on the backend.
  useEffect(() => {
    if (preview || !ready || !model || selected.current) return;
    if (settings && settings.selected_model === model.id) return;
    selected.current = true;
    selectModel(model.id);
  }, [preview, ready, model, settings, selectModel]);

  const progress = downloadProgress[id];
  return {
    ready,
    busy,
    failed: failed && !busy,
    percent: progress ? progress.percentage : null,
    downloaded: progress?.downloaded ?? 0,
    total: progress?.total ?? (model ? model.size_mb * 1024 * 1024 : 0),
    retry: () => setAttempt((a) => a + 1),
  };
}

const bytes = (n: number) =>
  n >= 1024 ** 3
    ? `${(n / 1024 ** 3).toFixed(1)} GB`
    : `${Math.round(n / 1024 ** 2)} MB`;

export const ModelRow: React.FC<{ model: MoqiModelState }> = ({ model }) => {
  const { t } = useTranslation();
  return (
    <div className="flex items-center gap-3 px-4 py-3.5">
      <span className="w-8 h-8 rounded-[8px] grid place-items-center text-white shrink-0 bg-[#3a3a3c]">
        <DownloadIcon size={17} />
      </span>
      <div className="flex-1 min-w-0 flex flex-col gap-1.5">
        <div className="flex items-baseline justify-between gap-2">
          <h3 className="m-0 text-[13px] font-semibold text-text">
            {t("moqi.onboarding.model")}
          </h3>
          {model.busy && model.percent !== null && (
            <span className="text-[12px] text-muted tabular-nums">
              {bytes(model.downloaded)} / {bytes(model.total)}
            </span>
          )}
        </div>
        {model.busy && (
          <span className="block h-1 rounded-sm bg-black/[0.08] dark:bg-white/10 overflow-hidden">
            <span
              className="block h-full rounded-sm bg-logo-primary transition-[width] duration-300"
              style={{ width: `${model.percent ?? 100}%` }}
            />
          </span>
        )}
        <p className="m-0 text-[12px] leading-snug text-muted">
          {model.failed
            ? t("moqi.onboarding.modelFailed")
            : model.busy && model.percent === null
              ? t("moqi.onboarding.modelPreparing")
              : t("moqi.onboarding.modelHint", {
                  size: model.total ? bytes(model.total) : "…",
                })}
        </p>
      </div>
      {model.ready ? (
        <span className="flex items-center gap-1 text-[13px] font-medium text-positive shrink-0">
          <CheckIcon size={14} />
          {t("moqi.onboarding.modelReady")}
        </span>
      ) : model.failed ? (
        <SmallButton onClick={model.retry}>
          {t("moqi.onboarding.retry")}
        </SmallButton>
      ) : null}
    </div>
  );
};
