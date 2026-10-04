import { useEffect, useState, useCallback, useRef } from "react";
import { useTranslation } from "react-i18next";
import { platform } from "@tauri-apps/plugin-os";
import {
  checkAccessibilityPermission,
  requestAccessibilityPermission,
  checkMicrophonePermission,
  requestMicrophonePermission,
} from "tauri-plugin-macos-permissions-api";
import { toast } from "sonner";
import { commands } from "@/bindings";
import { useSettingsStore } from "@/stores/settingsStore";
import { YuyinMark } from "@/yuyin/YuyinLogo"; // Yuyin fork
import { ModelRow, useMoqiModel } from "@/yuyin/onboarding/ModelRow"; // Yuyin fork
import { PrimaryButton } from "@/yuyin/window/ui"; // Yuyin fork
import { Accessibility, Mic, Check, Loader2 } from "lucide-react";

interface AccessibilityOnboardingProps {
  onComplete: () => void;
  preview?: boolean;
  /** Yuyin fork: first of three setup steps (new users). */
  showSteps?: boolean;
}

type PermissionStatus = "checking" | "needed" | "waiting" | "granted";
type PermissionPlatform = "macos" | "windows" | "other";

interface PermissionsState {
  accessibility: PermissionStatus;
  microphone: PermissionStatus;
}

const AccessibilityOnboarding: React.FC<AccessibilityOnboardingProps> = ({
  onComplete,
  preview = false,
  showSteps = false,
}) => {
  const { t } = useTranslation();
  const refreshAudioDevices = useSettingsStore(
    (state) => state.refreshAudioDevices,
  );
  const refreshOutputDevices = useSettingsStore(
    (state) => state.refreshOutputDevices,
  );
  const [permissionPlatform, setPermissionPlatform] =
    useState<PermissionPlatform | null>(null);
  const [permissions, setPermissions] = useState<PermissionsState>({
    accessibility: "checking",
    microphone: "checking",
  });
  // Yuyin fork: the speech model is the third thing to get ready. Users who
  // already had everything on launch pass straight through (autoComplete);
  // anyone who had to do something presses 繼續 when all three are done.
  const model = useMoqiModel(preview);
  const [autoComplete, setAutoComplete] = useState(false);
  const pollingRef = useRef<ReturnType<typeof setInterval> | null>(null);
  const timeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const errorCountRef = useRef<number>(0);
  const MAX_POLLING_ERRORS = 3;

  const isMacOS = permissionPlatform === "macos";
  const isWindows = permissionPlatform === "windows";
  const showMicrophonePermission = isMacOS || isWindows;
  const showAccessibilityPermission = isMacOS;

  const allGranted = isMacOS
    ? permissions.accessibility === "granted" &&
      permissions.microphone === "granted"
    : isWindows
      ? permissions.microphone === "granted"
      : true;

  const completeOnboarding = useCallback(async () => {
    await Promise.all([refreshAudioDevices(), refreshOutputDevices()]);
    timeoutRef.current = setTimeout(() => onComplete(), 300);
  }, [onComplete, refreshAudioDevices, refreshOutputDevices]);

  const hasWindowsMicrophoneAccess = useCallback(async (): Promise<boolean> => {
    const microphoneStatus =
      await commands.getWindowsMicrophonePermissionStatus();

    if (!microphoneStatus.supported) {
      return true;
    }

    return microphoneStatus.overall_access !== "denied";
  }, []);

  // Check platform and permission status on mount
  useEffect(() => {
    const currentPlatform = platform();
    const nextPlatform: PermissionPlatform =
      currentPlatform === "macos"
        ? "macos"
        : currentPlatform === "windows"
          ? "windows"
          : "other";

    setPermissionPlatform(nextPlatform);

    // Debug previews are intentionally inert: show the permission request UI
    // without checking or changing operating-system permissions.
    if (preview) {
      setPermissions({
        accessibility: nextPlatform === "macos" ? "needed" : "granted",
        microphone: nextPlatform === "other" ? "granted" : "needed",
      });
      return;
    }

    // Skip immediately on unsupported platforms
    if (nextPlatform === "other") {
      onComplete();
      return;
    }

    const checkInitial = async () => {
      if (nextPlatform === "macos") {
        try {
          const [accessibilityGranted, microphoneGranted] = await Promise.all([
            checkAccessibilityPermission(),
            checkMicrophonePermission(),
          ]);

          // If accessibility is granted, initialize Enigo and shortcuts
          if (accessibilityGranted) {
            try {
              await Promise.all([
                commands.initializeEnigo(),
                commands.initializeShortcuts(),
              ]);
            } catch (e) {
              console.warn("Failed to initialize after permission grant:", e);
            }
          }

          const newState: PermissionsState = {
            accessibility: accessibilityGranted ? "granted" : "needed",
            microphone: microphoneGranted ? "granted" : "needed",
          };

          setPermissions(newState);

          if (accessibilityGranted && microphoneGranted) {
            setAutoComplete(true); // Yuyin fork: once the model is ready too
          }
        } catch (error) {
          console.error("Failed to check macOS permissions:", error);
          toast.error(t("onboarding.permissions.errors.checkFailed"));
          setPermissions({
            accessibility: "needed",
            microphone: "needed",
          });
        }

        return;
      }

      // Yuyin fork: Windows needs no permission for the keyboard hook or for
      // pasting, so start them now, as macOS does once Accessibility is on.
      // Step 3 (試說一句) needs the talk key before onboarding is "done".
      Promise.all([
        commands.initializeEnigo(),
        commands.initializeShortcuts(),
      ]).catch((e) => {
        console.warn("Failed to initialize shortcuts:", e);
      });

      try {
        const microphoneGranted = await hasWindowsMicrophoneAccess();

        setPermissions({
          accessibility: "granted",
          microphone: microphoneGranted ? "granted" : "needed",
        });

        if (microphoneGranted) {
          setAutoComplete(true); // Yuyin fork
        }
      } catch (error) {
        console.warn("Failed to check Windows microphone permissions:", error);
        setPermissions({
          accessibility: "granted",
          microphone: "granted",
        });
        setAutoComplete(true); // Yuyin fork
      }
    };

    checkInitial();
  }, [completeOnboarding, hasWindowsMicrophoneAccess, onComplete, preview, t]);

  // Polling for permissions after user clicks a button
  const startPolling = useCallback(() => {
    if (pollingRef.current || permissionPlatform === null) return;

    pollingRef.current = setInterval(async () => {
      try {
        if (permissionPlatform === "windows") {
          const microphoneGranted = await hasWindowsMicrophoneAccess();

          if (microphoneGranted) {
            setPermissions((prev) => ({ ...prev, microphone: "granted" }));

            if (pollingRef.current) {
              clearInterval(pollingRef.current);
              pollingRef.current = null;
            }
            // Yuyin fork: the user continues with 繼續.
          }

          errorCountRef.current = 0;
          return;
        }

        const [accessibilityGranted, microphoneGranted] = await Promise.all([
          checkAccessibilityPermission(),
          checkMicrophonePermission(),
        ]);

        setPermissions((prev) => {
          const newState = { ...prev };

          if (accessibilityGranted && prev.accessibility !== "granted") {
            newState.accessibility = "granted";
            // Initialize Enigo and shortcuts when accessibility is granted
            Promise.all([
              commands.initializeEnigo(),
              commands.initializeShortcuts(),
            ]).catch((e) => {
              console.warn("Failed to initialize after permission grant:", e);
            });
          }

          if (microphoneGranted && prev.microphone !== "granted") {
            newState.microphone = "granted";
          }

          return newState;
        });

        // If both granted, stop polling, refresh audio devices, and proceed
        if (accessibilityGranted && microphoneGranted) {
          if (pollingRef.current) {
            clearInterval(pollingRef.current);
            pollingRef.current = null;
          }
          // Yuyin fork: the user continues with 繼續.
        }

        // Reset error count on success
        errorCountRef.current = 0;
      } catch (error) {
        console.error("Error checking permissions:", error);
        errorCountRef.current += 1;

        if (errorCountRef.current >= MAX_POLLING_ERRORS) {
          // Stop polling after too many consecutive errors
          if (pollingRef.current) {
            clearInterval(pollingRef.current);
            pollingRef.current = null;
          }
          toast.error(t("onboarding.permissions.errors.checkFailed"));
        }
      }
    }, 1000);
  }, [completeOnboarding, hasWindowsMicrophoneAccess, permissionPlatform, t]);

  // Yuyin fork: everything was already in place on launch.
  useEffect(() => {
    if (autoComplete && model.ready) completeOnboarding();
  }, [autoComplete, model.ready, completeOnboarding]);

  // Cleanup polling and timeouts on unmount
  useEffect(() => {
    return () => {
      if (pollingRef.current) {
        clearInterval(pollingRef.current);
      }
      if (timeoutRef.current) {
        clearTimeout(timeoutRef.current);
      }
    };
  }, []);

  const handleGrantAccessibility = async () => {
    if (preview) return;

    try {
      await requestAccessibilityPermission();
      setPermissions((prev) => ({ ...prev, accessibility: "waiting" }));
      startPolling();
    } catch (error) {
      console.error("Failed to request accessibility permission:", error);
      toast.error(t("onboarding.permissions.errors.requestFailed"));
    }
  };

  const handleGrantMicrophone = async () => {
    if (preview) return;

    try {
      if (isWindows) {
        await commands.openMicrophonePrivacySettings();
      } else {
        await requestMicrophonePermission();
      }

      setPermissions((prev) => ({ ...prev, microphone: "waiting" }));
      startPolling();
    } catch (error) {
      console.error("Failed to request microphone permission:", error);
      toast.error(t("onboarding.permissions.errors.requestFailed"));
    }
  };

  const isChecking =
    permissionPlatform === null ||
    (isMacOS &&
      permissions.accessibility === "checking" &&
      permissions.microphone === "checking") ||
    (isWindows && permissions.microphone === "checking");

  // Still checking platform/initial permissions
  if (isChecking) {
    return (
      <div className="h-screen w-full flex items-center justify-center">
        <Loader2 className="w-8 h-8 animate-spin text-text/50" />
      </div>
    );
  }

  // Yuyin fork: an Apple-style welcome — the app icon, one sentence on what
  // the app does, and a grouped list: the two permissions and the model.
  const ready = allGranted && model.ready;

  const renderRow = (
    Icon: typeof Mic,
    color: string,
    title: string,
    description: string,
    status: PermissionStatus,
    onGrant: () => void,
    grantLabel: string,
  ) => (
    <div className="flex items-center gap-3 px-4 py-3.5">
      <span
        className="w-8 h-8 rounded-[8px] grid place-items-center text-white shrink-0"
        style={{ background: color }}
      >
        <Icon className="w-[18px] h-[18px]" strokeWidth={2.2} />
      </span>
      <div className="flex-1 min-w-0">
        <h3 className="text-[13px] font-semibold text-text">{title}</h3>
        <p className="text-[12px] text-text/60 leading-snug mt-0.5">
          {description}
        </p>
      </div>
      {status === "granted" ? (
        <span className="flex items-center gap-1 text-[13px] font-medium text-[#30d158] shrink-0">
          <Check className="w-4 h-4" strokeWidth={2.6} />
          {t("onboarding.permissions.granted")}
        </span>
      ) : status === "waiting" ? (
        <span className="flex items-center gap-1.5 text-[12px] text-text/50 shrink-0">
          <Loader2 className="w-3.5 h-3.5 animate-spin" />
          {t("onboarding.permissions.waiting")}
        </span>
      ) : (
        <button
          onClick={onGrant}
          className="px-3.5 py-[5px] rounded-full bg-logo-primary text-white text-[13px] font-medium hover:brightness-110 active:brightness-95 transition shrink-0"
        >
          {grantLabel}
        </button>
      )}
    </div>
  );

  return (
    <div className="h-screen w-full flex flex-col items-center justify-center p-8 bg-background">
      <div data-tauri-drag-region className="fixed top-0 inset-x-0 h-[52px]" />
      {showSteps && (
        <div
          className="fixed top-[22px] end-6 flex gap-1.5"
          aria-label={t("moqi.onboarding.step", { n: 1, total: 3 })}
        >
          <span className="w-[18px] h-1 rounded-sm bg-text" />
          <span className="w-[18px] h-1 rounded-sm bg-black/15 dark:bg-white/20" />
          <span className="w-[18px] h-1 rounded-sm bg-black/15 dark:bg-white/20" />
        </div>
      )}
      <div className="w-full max-w-[460px] flex flex-col items-center">
        <YuyinMark
          size={76}
          className="drop-shadow-[0_10px_20px_rgba(0,0,0,0.22)]"
        />
        <h1 className="mt-5 text-[26px] font-bold tracking-[-0.02em] text-text">
          {t("onboarding.permissions.welcome")}
        </h1>
        <p className="mt-2 text-[14px] text-text/65 text-center leading-relaxed whitespace-pre-line [word-break:keep-all] [text-wrap:balance]">
          {t("onboarding.permissions.intro")}
        </p>
        <div className="mt-7 w-full bg-surface rounded-[12px] divide-y divide-hairline shadow-[0_0_0_0.5px_var(--color-hairline),0_1px_3px_rgba(0,0,0,0.05)]">
          {showMicrophonePermission &&
            renderRow(
              Mic,
              "#ff9500",
              t("onboarding.permissions.microphone.title"),
              t("onboarding.permissions.microphone.description"),
              permissions.microphone,
              handleGrantMicrophone,
              isWindows
                ? t("accessibility.openSettings")
                : t("onboarding.permissions.grant"),
            )}
          {showAccessibilityPermission &&
            renderRow(
              Accessibility,
              "#007aff",
              t("onboarding.permissions.accessibility.title"),
              t("onboarding.permissions.accessibility.description"),
              permissions.accessibility,
              handleGrantAccessibility,
              t("onboarding.permissions.grant"),
            )}
          <ModelRow model={model} />
        </div>
        <p className="mt-4 text-[12px] text-muted text-center">
          {t("onboarding.permissions.privacy")}
        </p>
        <PrimaryButton
          className="mt-5"
          disabled={!ready}
          onClick={() => completeOnboarding()}
        >
          {t("moqi.onboarding.continue")}
        </PrimaryButton>
        {!ready && (
          <span className="mt-2 text-[11px] text-muted">
            {t("moqi.onboarding.continueHint")}
          </span>
        )}
      </div>
    </div>
  );
};

export default AccessibilityOnboarding;
