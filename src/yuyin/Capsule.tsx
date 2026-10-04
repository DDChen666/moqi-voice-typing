// Yuyin fork: the recording capsule that replaces Handy's compact pill.
// Design and motion spec: 設計文件/介面/Yuyin_介面提案_v2.html.
import { listen } from "@tauri-apps/api/event";
import React, { useEffect, useLayoutEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import "./capsule.css";
import { reducedMotion } from "./springs";

export type CapsuleState =
  | "recording"
  | "transcribing"
  | "processing"
  | "done"
  | "fallback"
  | "copied";

type WritingContext = "chat" | "to_ai" | "notes" | "other";
interface ContextEvent {
  context: WritingContext;
  app: string;
  /** Double-tap: the key no longer ends the recording, the next press does. */
  hands_free: boolean;
  /** Where this dictation's text goes ("DeepSeek"); null: nothing leaves. */
  sends_to: string | null;
  /** What the user says edits the text they selected. */
  editing: boolean;
  /** The text will be written in another language. */
  translating: boolean;
}

const BARS = 13;
// Taller in the middle, like a voice envelope.
const ENVELOPE = [
  0.25, 0.4, 0.58, 0.74, 0.88, 0.97, 1, 0.97, 0.88, 0.74, 0.58, 0.4, 0.25,
];
// The waveform's design size in CSS px (Retina): 2 px bars, 2.5 px apart,
// 2–19 px tall in a 20 px row.
const BAR_W = 2;
const BAR_GAP = 2.5;
const WAVE_H = 20;
// Loudness the bars are scaled against (see the frame loop). Kept between
// recordings so the first word is already the right size; this first guess
// sits between a laptop's microphone and a quiet USB one.
let loudness = 0.5;
const MIN_LOUDNESS = 0.3; // caps the boost at about 3×, so pauses stay flat
const NOISE_GATE = 0.04;
const LOUDNESS_FALLOFF = 0.84; // per second: a loud word fades out of it in ~4 s
const INTRO_MS = 1400;
const POLISH_LABEL_DELAY_MS = 300; // criterion 2: only say "tidying" if it takes a while
const TIMER_AFTER_S = 10; // criterion 11

/** The waveform in whole device pixels, so every bar is equally sharp at any
 * display scale. At 2× this is the design (4 px bars, 5 px gaps); at 1× a
 * 2 px bar fell on uneven pixel columns and looked blocky, so it gets 3. */
const waveGeometry = (dpr: number) => {
  const bar = Math.max(3, Math.round(BAR_W * dpr));
  const gap = Math.max(2, Math.round(BAR_GAP * dpr));
  return {
    dpr,
    bar,
    gap,
    width: BARS * bar + (BARS - 1) * gap,
    height: Math.round(WAVE_H * dpr),
  };
};

const ContextIcon: React.FC<{ context: WritingContext }> = ({ context }) => {
  switch (context) {
    case "chat":
      return (
        <svg viewBox="0 0 16 16" aria-hidden="true">
          <path
            d="M8 2.5c3.4 0 6 2.1 6 4.8S11.4 12 8 12c-.6 0-1.1-.05-1.6-.16L3.3 13.3l.7-2.2C2.7 10.3 2 9 2 7.3 2 4.6 4.6 2.5 8 2.5Z"
            fill="none"
            stroke="currentColor"
            strokeWidth="1.4"
            strokeLinejoin="round"
          />
        </svg>
      );
    case "to_ai":
      return (
        <svg viewBox="0 0 16 16" aria-hidden="true">
          <path
            d="M8 1.8l1.3 3.6 3.6 1.3-3.6 1.3L8 11.6 6.7 8 3.1 6.7l3.6-1.3L8 1.8Zm4.4 8.4.5 1.4 1.4.5-1.4.5-.5 1.4-.5-1.4-1.4-.5 1.4-.5.5-1.4Z"
            fill="currentColor"
          />
        </svg>
      );
    case "notes":
      return (
        <svg viewBox="0 0 16 16" aria-hidden="true">
          <path
            d="M3.6 2h6l2.8 2.8v8.7c0 .3-.2.5-.5.5H3.6a.5.5 0 0 1-.5-.5V2.5c0-.3.2-.5.5-.5Zm1.6 5h5.6M5.2 9.4h5.6M5.2 11.7h3.6"
            fill="none"
            stroke="currentColor"
            strokeWidth="1.4"
            strokeLinecap="round"
            strokeLinejoin="round"
          />
        </svg>
      );
    default:
      return (
        <svg viewBox="0 0 16 16" aria-hidden="true">
          <path
            d="M6 3h4M8 3v10M6 13h4"
            fill="none"
            stroke="currentColor"
            strokeWidth="1.4"
            strokeLinecap="round"
          />
        </svg>
      );
  }
};

interface CapsuleProps {
  state: CapsuleState;
  visible: boolean;
  /** Microphone samples are flowing (the start cue has played). */
  captureReady: boolean;
  /** Seconds recorded so far. */
  elapsed: number;
  direction: "ltr" | "rtl";
}

export const Capsule: React.FC<CapsuleProps> = ({
  state,
  visible,
  captureReady,
  elapsed,
  direction,
}) => {
  const { t } = useTranslation();
  const capRef = useRef<HTMLDivElement>(null);
  const shellRef = useRef<HTMLDivElement>(null);
  const waveRef = useRef<HTMLCanvasElement>(null);
  const targetLevel = useRef(0);
  const [shown, setShown] = useState(false);
  const [ctx, setCtx] = useState<ContextEvent | null>(null);
  const [intro, setIntro] = useState(false);
  const [polishing, setPolishing] = useState(false);

  // Microphone level (16 FFT buckets) and the writing context from the backend.
  useEffect(() => {
    let introTimer: ReturnType<typeof setTimeout> | undefined;
    const unlisten = [
      listen<number[]>("mic-level", (e) => {
        const b = e.payload;
        let sum = 0;
        for (let i = 0; i < Math.min(10, b.length); i++) sum += b[i] || 0;
        targetLevel.current = Math.min(1, (sum / 10) * 1.8);
      }),
      listen<ContextEvent>("yuyin-context", (e) => {
        setCtx(e.payload);
        setIntro(true);
        clearTimeout(introTimer);
        introTimer = setTimeout(() => setIntro(false), INTRO_MS);
      }),
    ];
    return () => {
      clearTimeout(introTimer);
      unlisten.forEach((p) => p.then((fn) => fn()));
    };
  }, []);

  // Enter on the next frame so the transition runs from the collapsed state.
  useEffect(() => {
    if (!visible) {
      setShown(false);
      return;
    }
    const id = requestAnimationFrame(() => setShown(true));
    return () => cancelAnimationFrame(id);
  }, [visible]);

  // Say "tidying" only when the clean-up takes longer than 300 ms.
  useEffect(() => {
    setPolishing(false);
    if (state !== "processing") return;
    const id = setTimeout(() => setPolishing(true), POLISH_LABEL_DELAY_MS);
    return () => clearTimeout(id);
  }, [state]);

  // Waveform and rim: one damped spring per bar, driven by the mic level.
  useEffect(() => {
    if (!visible || state !== "recording") return;
    const canvas = waveRef.current;
    const pen = canvas?.getContext("2d") ?? null;
    const g = waveGeometry(window.devicePixelRatio || 1);
    if (canvas) {
      canvas.width = g.width;
      canvas.height = g.height;
    }
    const bars = Array.from({ length: BARS }, () => ({ h: 2, v: 0, j: 1 }));
    let level = 0;
    let angle = 0;
    let alpha = 0.35;
    let last = performance.now();
    let raf = 0;
    const frame = (now: number) => {
      const dt = Math.min(0.1, Math.max(0, now - last) / 1000);
      last = now;
      // A quiet microphone (a USB mic at arm's length reads ~20 dB below a
      // laptop's) barely moved the bars. Scale against the recent loudness
      // instead: it jumps to each new peak and fades over a few seconds, so
      // a soft voice fills the bars like a loud one.
      const raw = targetLevel.current;
      loudness = Math.max(
        raw,
        MIN_LOUDNESS,
        loudness * Math.pow(LOUDNESS_FALLOFF, dt),
      );
      const scaled = Math.min(
        1,
        Math.max(0, raw - NOISE_GATE) / (loudness - NOISE_GATE),
      );
      level += (scaled - level) * 0.16;
      const cap = capRef.current;
      if (cap) {
        cap.style.setProperty("--level", level.toFixed(3));
        if (!reducedMotion) {
          angle = (angle + 0.25 + level * 1.1) % 360;
          cap.style.setProperty("--angle", `${angle.toFixed(1)}deg`);
        }
      }
      bars.forEach((b, i) => {
        if (Math.random() < 0.08) b.j = 0.65 + Math.random() * 0.7;
        const goal = 2 + level * ENVELOPE[i] * b.j * 17;
        b.v = (b.v + (goal - b.h) * 0.22) * 0.62;
        b.h = Math.max(2, Math.min(19, b.h + b.v));
      });
      if (pen) {
        // Dim while silent, bright while speaking (fades in ~0.4 s).
        alpha += ((level > 0.1 ? 0.95 : 0.35) - alpha) * Math.min(1, dt / 0.13);
        const r = g.bar / 2;
        pen.clearRect(0, 0, g.width, g.height);
        pen.fillStyle = `rgba(245, 245, 247, ${alpha.toFixed(3)})`;
        pen.beginPath();
        bars.forEach((b, i) => {
          const h = Math.max(g.bar, b.h * g.dpr);
          const x = i * (g.bar + g.gap);
          const y = (g.height - h) / 2;
          pen.moveTo(x, y + r);
          pen.arc(x + r, y + r, r, Math.PI, 0);
          pen.lineTo(x + g.bar, y + h - r);
          pen.arc(x + r, y + h - r, r, 0, Math.PI);
          pen.closePath();
        });
        pen.fill();
      }
      raf = requestAnimationFrame(frame);
    };
    raf = requestAnimationFrame(frame);
    return () => {
      cancelAnimationFrame(raf);
      targetLevel.current = 0;
    };
  }, [visible, state]);

  const recording = state === "recording";
  const wave = waveGeometry(window.devicePixelRatio || 1);
  const working = state === "transcribing" || state === "processing";
  const showTimer = recording && elapsed >= TIMER_AFTER_S;
  const handsFree = recording && ctx?.hands_free === true;
  // The app's name shows briefly; "editing the selection" stays, so the user
  // knows what they say will replace it.
  const showCtx =
    recording && ctx !== null && (ctx.editing || (intro && ctx.app.length > 0));

  // Morph the width: measure the content, then spring the capsule to it.
  useLayoutEffect(() => {
    const shell = shellRef.current;
    const cap = capRef.current;
    if (!shell || !cap) return;
    shell.style.width = "max-content";
    const width = shell.offsetWidth; // layout width, unaffected by transforms
    shell.style.width = "";
    cap.style.width = `${width}px`;
  }, [state, showCtx, showTimer, handsFree, polishing, ctx]);

  const classes = [
    "yy-cap",
    state,
    shown && visible ? "show" : "",
    !visible ? "leave" : "",
    captureReady ? "ready" : "",
    polishing ? "polishing" : "",
  ]
    .filter(Boolean)
    .join(" ");

  const fmt = (s: number) =>
    `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;

  return (
    <div className="yy-stage" dir={direction}>
      <div className={classes} ref={capRef} aria-live="polite">
        <div className="yy-shell" ref={shellRef}>
          {recording && (
            <>
              <span className="yy-rec" />
              <canvas
                className="yy-wave"
                ref={waveRef}
                aria-hidden="true"
                style={{
                  width: wave.width / wave.dpr,
                  height: wave.height / wave.dpr,
                }}
              />
              {showCtx && ctx && (
                <span className="yy-ctx">
                  <ContextIcon context={ctx.context} />
                  <span className="yy-clip">
                    {ctx.editing ? t("overlay.yuyin.editSelection") : ctx.app}
                  </span>
                </span>
              )}
              {handsFree && !showCtx && (
                <span className="yy-hands">{t("overlay.yuyin.handsFree")}</span>
              )}
              {showTimer && <span className="yy-timer">{fmt(elapsed)}</span>}
            </>
          )}
          {working && (
            <>
              <span className="yy-line" />
              {polishing && (
                <span className="yy-status">
                  {ctx?.editing
                    ? t("overlay.yuyin.editing")
                    : ctx?.translating
                      ? t("overlay.yuyin.translating")
                      : t("overlay.yuyin.polishing")}
                </span>
              )}
              {polishing && ctx?.sends_to && (
                <span className="yy-dest">
                  <svg viewBox="0 0 16 16" aria-hidden="true">
                    <path
                      d="M8 13V3.5M4.2 7.2 8 3.4l3.8 3.8"
                      fill="none"
                      stroke="currentColor"
                      strokeWidth="1.8"
                      strokeLinecap="round"
                      strokeLinejoin="round"
                    />
                  </svg>
                  <span className="yy-clip">
                    {t("overlay.yuyin.sendsTo", { to: ctx.sends_to })}
                  </span>
                </span>
              )}
            </>
          )}
          {state === "done" && (
            <svg className="yy-check" viewBox="0 0 16 16" aria-hidden="true">
              <defs>
                <linearGradient id="yy-prism" x1="0" y1="0" x2="1" y2="0">
                  <stop offset="0" stopColor="#c9d7ff" />
                  <stop offset="0.5" stopColor="#f5f5f7" />
                  <stop offset="1" stopColor="#ffd9e6" />
                </linearGradient>
              </defs>
              <path d="M3.5 8.4l3 3 6-6.6" stroke="url(#yy-prism)" />
            </svg>
          )}
          {state === "done" && ctx && !ctx.sends_to && (
            <span className="yy-dest">
              <svg viewBox="0 0 16 16" aria-hidden="true">
                <rect
                  x="3.2"
                  y="7"
                  width="9.6"
                  height="6.8"
                  rx="1.6"
                  fill="none"
                  stroke="currentColor"
                  strokeWidth="1.8"
                />
                <path
                  d="M5.4 7V5.2a2.6 2.6 0 0 1 5.2 0V7"
                  fill="none"
                  stroke="currentColor"
                  strokeWidth="1.8"
                />
              </svg>
              {t("overlay.yuyin.allLocal")}
            </span>
          )}
          {state === "fallback" && (
            <>
              <span className="yy-amber" />
              <span className="yy-status">{t("overlay.yuyin.fallback")}</span>
            </>
          )}
          {state === "copied" && (
            <span className="yy-status">
              <svg viewBox="0 0 16 16" aria-hidden="true">
                <path
                  d="M5.5 3H4a1 1 0 0 0-1 1v9.5a1 1 0 0 0 1 1h8a1 1 0 0 0 1-1V4a1 1 0 0 0-1-1h-1.5M6 1.8h4v2.4H6z"
                  fill="none"
                  stroke="currentColor"
                  strokeWidth="1.4"
                  strokeLinejoin="round"
                />
              </svg>
              {t("overlay.copied")}
            </span>
          )}
        </div>
      </div>
    </div>
  );
};
