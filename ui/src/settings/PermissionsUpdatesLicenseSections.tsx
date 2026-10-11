// Permissions, Updates, and License sections, ported from Envious Wispr's
// PermissionsSettingsView state machine onto Teletype's primitives.
//
// The permission row is the whole point: the action button's label and
// destination depend on the tri-state, because macOS behaves differently per
// state. A permission never requested still prompts on demand, but a refused
// one never prompts again, so a single "Grant Access" button either nags
// forever or sends someone to System Settings who only needed to click Allow.

import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { getVersion } from "@tauri-apps/api/app";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import {
  Chip,
  PrimaryButton,
  Row,
  SecondaryButton,
  Section,
} from "./primitives";
import type { Settings } from "../screens/SettingsScreen";

// The parent screen owns the Settings object; these props mirror the other
// ported sections so the integrator can wire this one the same way. Neither
// is used by the controls here, which read live platform state.

type PermissionState = "granted" | "denied" | "notDetermined" | "unsupported";
type PermissionKind = "microphone" | "accessibility";

interface Permission {
  kind: PermissionKind;
  state: PermissionState;
}

interface PermissionMeta {
  name: string;
  purpose: string;
}

const PERMISSION_META: Record<PermissionKind, PermissionMeta> = {
  microphone: {
    name: "Microphone",
    purpose: "Captures your voice so it can be transcribed.",
  },
  accessibility: {
    name: "Accessibility",
    purpose: "Reads and types into your focused app so dictation lands where you are working.",
  },
};

const STATE_CHIP: Record<PermissionState, { label: string; tone: "success" | "warning" | "danger" | "neutral" }> = {
  granted: { label: "Granted", tone: "success" },
  notDetermined: { label: "Not requested", tone: "neutral" },
  denied: { label: "Denied", tone: "danger" },
  unsupported: { label: "Not applicable", tone: "neutral" },
};

// Granting happens in System Settings, so the screen notices the change when
// the user comes back. A 2 s cadence is coarse enough to be free; the TCC
// lookups are cheap once the window is back in front.
const POLL_MS = 2000;

export function PermissionsSection(props: {
  settings: Settings;
  onChange: (patch: Partial<Settings>) => void;
}) {
  // Permissions are platform state, not persisted settings: the section
  // fetches them itself and keeps them fresh while the window is focused.
  void props;
  const [permissions, setPermissions] = useState<Permission[] | null>(null);

  const refresh = useCallback(() => {
    invoke<Permission[]>("get_permissions")
      .then(setPermissions)
      .catch((e) => console.error("get_permissions failed", e));
  }, []);

  useEffect(() => {
    refresh();
    let timer: number | undefined;

    const tick = () => {
      // Only poll while the window is in front. get_permissions is not free on
      // macOS: the accessibility check spawns a thread for an AX round trip,
      // so a hidden window ticking every 2 s for hours is real work for a
      // screen nobody is looking at. Focus and visibility handlers below do
      // the refresh the moment the user comes back.
      if (!document.hidden) refresh();
      timer = window.setTimeout(tick, POLL_MS);
    };
    const onFocus = () => {
      // The user may have just toggled a permission in System Settings.
      refresh();
    };
    window.addEventListener("focus", onFocus);
    document.addEventListener("visibilitychange", onFocus);
    timer = window.setTimeout(tick, POLL_MS);

    let unlistenFocus: (() => void) | undefined;
    getCurrentWebviewWindow()
      .onFocusChanged(({ payload: focused }) => {
        if (focused) refresh();
      })
      .then((fn) => (unlistenFocus = fn));

    return () => {
      window.clearTimeout(timer);
      window.removeEventListener("focus", onFocus);
      document.removeEventListener("visibilitychange", onFocus);
      unlistenFocus?.();
    };
  }, [refresh]);

  const act = (kind: PermissionKind, state: PermissionState) => {
    if (state === "notDetermined") {
      invoke("request_permission", { kind })
        .catch((e) => console.error("request_permission failed", e))
        .then(refresh);
    } else if (state === "denied") {
      invoke("open_permission_settings", { kind }).catch((e) =>
        console.error("open_permission_settings failed", e)
      );
    } else if (state === "granted") {
      refresh();
    }
  };

  const byKind = (kind: PermissionKind) =>
    permissions?.find((p) => p.kind === kind) ?? null;

  return (
    <Section
      title="Permissions"
      hint="Both permissions are required for dictation to complete. If either is missing, you can speak but the words cannot be captured or inserted."
    >
      {(["microphone", "accessibility"] as PermissionKind[]).map((kind, i) => {
        const meta = PERMISSION_META[kind];
        const perm = byKind(kind);
        const state = perm?.state ?? "unsupported";
        const chip = STATE_CHIP[state];
        return (
          <Row
            key={kind}
            first={i === 0}
            label={meta.name}
            hint={
              kind === "accessibility" && state === "denied"
                ? "macOS will guide you to the right toggle in System Settings."
                : meta.purpose
            }
          >
            <Chip tone={chip.tone}>{chip.label}</Chip>
            {state === "unsupported" ? null : (
              state === "notDetermined" ? (
                <PrimaryButton onClick={() => act(kind, state)}>Request Access</PrimaryButton>
              ) : state === "denied" ? (
                <SecondaryButton onClick={() => act(kind, state)}>Open System Settings</SecondaryButton>
              ) : (
                <SecondaryButton onClick={() => act(kind, state)}>Re-check</SecondaryButton>
              )
            )}
          </Row>
        );
      })}
    </Section>
  );
}

const RELEASES_URL = "https://github.com/alihusains/teletype/releases";

// The updater's full lifecycle, from the user's point of view. Every state has
// an explicit UI below: nothing silently falls through to the previous one.
type UpdateState =
  | { phase: "idle"; version: string | null }
  | { phase: "checking"; version: string | null }
  | { phase: "current"; version: string | null }
  | { phase: "available"; version: string | null; update: Update }
  | { phase: "downloading"; version: string | null; update: Update; downloaded: number; total?: number }
  | { phase: "ready"; version: string | null; update: Update }
  | { phase: "restarting"; version: string | null }
  | { phase: "error"; version: string | null; message: string };

function describeUpdateError(e: unknown): string {
  const msg =
    typeof e === "string"
      ? e
      : e instanceof Error
        ? e.message
        : JSON.stringify(e);
  // Tauri's updater errors are verbose (the full endpoint URL is embedded).
  // Map the common failure classes to something a user can act on.
  if (/not found|404|failed to fetch|network|dns|timed out|timeout/i.test(msg)) {
    return "Could not reach the update server. Check your connection and try again.";
  }
  if (/signature|minisign|verification|invalid|corrupt/i.test(msg)) {
    return "The update could not be verified. Do not install it; contact the maintainer if this persists.";
  }
  if (/not signed|unsigned/i.test(msg)) {
    return "This release is not signed with the update key, so it cannot be installed. The next signed release will update normally.";
  }
  if (/already|running|in progress/i.test(msg)) {
    return "Another update is already in progress. Wait for it to finish.";
  }
  return `Update failed: ${msg}`;
}

export function UpdatesSection(props: { version?: string }) {
  const [state, setState] = useState<UpdateState>({ phase: "idle", version: props.version ?? null });
  // Install is a two-step explicit confirmation: the user sees "Download &
  // install", then a confirm dialog before the app relaunches. `confirmInstall`
  // flips the second step on. Refs (not state) guard the in-flight operations
  // so a double-click or an unmount mid-flight cannot start a second check.
  const runningRef = useRef(false);
  const [autoCheck, setAutoCheck] = useState<boolean>(() => {
    try {
      const raw = localStorage.getItem("teletype.autoCheckUpdates");
      return raw === null ? true : raw === "1";
    } catch {
      return true;
    }
  });
  // Whether the silent on-launch check has already run this session (so a
  // toggle of "Check on launch" cannot fire a second check mid-session).
  const launchCheckDoneRef = useRef(false);

  const setAutoCheckPersisted = (v: boolean) => {
    setAutoCheck(v);
    try {
      localStorage.setItem("teletype.autoCheckUpdates", v ? "1" : "0");
    } catch {
      // ignore quota / private mode
    }
  };

  useEffect(() => {
    if (props.version) return;
    getVersion()
      .then((v) => setState((s) => ({ ...s, version: v })))
      .catch((e) => console.error("getVersion failed", e));
  }, [props.version]);

  const busy =
    state.phase === "checking" ||
    state.phase === "downloading" ||
    state.phase === "restarting";

  const checkForUpdates = useCallback(async () => {
    if (runningRef.current) return;
    runningRef.current = true;
    setState((s) => ({ phase: "checking", version: s.version }));
    try {
      const update = await check();
      if (update) {
        setState((s) => ({ phase: "available", version: s.version, update }));
      } else {
        setState((s) => ({ phase: "current", version: s.version }));
      }
    } catch (e) {
      console.error("update check failed", e);
      setState((s) => ({
        phase: "error",
        version: s.version,
        message: describeUpdateError(e),
      }));
    } finally {
      runningRef.current = false;
    }
  }, []);

  // One silent check on first mount, so the user sees the real status without
  // having to press anything. Manual checking always works regardless.
  useEffect(() => {
    if (!autoCheck || launchCheckDoneRef.current) return;
    launchCheckDoneRef.current = true;
    void checkForUpdates();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const downloadAndInstall = useCallback(async () => {
    if (runningRef.current || state.phase !== "available") return;
    const update = state.update;
    runningRef.current = true;
    setState((s) => ({
      phase: "downloading",
      version: s.version,
      update,
      downloaded: 0,
      total: undefined,
    }));
    try {
      await update.download((event) => {
        setState((s) => {
          if (s.phase !== "downloading") return s;
          if (event.event === "Started" && event.data.contentLength) {
            return { ...s, total: event.data.contentLength };
          }
          if (event.event === "Progress") {
            return { ...s, downloaded: s.downloaded + event.data.chunkLength };
          }
          return s;
        });
      });
      setState((s) => ({ phase: "ready", version: s.version, update }));
    } catch (e) {
      console.error("update download failed", e);
      try {
        await update.close();
      } catch {
        // resource may already be gone
      }
      setState((s) => ({
        phase: "error",
        version: s.version,
        message: describeUpdateError(e),
      }));
    } finally {
      runningRef.current = false;
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [state.phase]);

  // Reads `state` only through the phase guards above; the Update resource is
  // captured from the click that started the flow.
  const installAndRestart = useCallback(async () => {
    if (state.phase !== "ready") return;
    const update = state.update;
    setState((s) => ({ phase: "restarting", version: s.version }));
    try {
      await update.install();
      await relaunch();
    } catch (e) {
      console.error("update install failed", e);
      setState((s) => ({
        phase: "error",
        version: s.version,
        message: describeUpdateError(e),
      }));
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [state.phase]);

  // If the user closes settings while an update is staged but not installed,
  // release the held bytes so the backend does not keep them for nothing.
  useEffect(() => {
    return () => {
      if (state.phase === "available" || state.phase === "ready") {
        void state.update.close().catch(() => {});
      }
    };
    // Only on unmount: reading `state` in deps would close a freshly staged
    // update the moment the phase changes.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const pct =
    state.phase === "downloading" && state.total
      ? Math.min(100, Math.round((state.downloaded / state.total) * 100))
      : null;

  return (
    <Section
      title="Updates"
      hint="Teletype checks the signed release manifest on GitHub Releases. Updates are downloaded and verified against the release signing key before anything is installed, and the app only restarts when you confirm."
    >
      <Row first label="Version" hint="The version of Teletype running right now.">
        <Chip>{state.version ?? "unknown"}</Chip>
      </Row>
      <Row
        label="Check for updates"
        hint={
          state.phase === "current"
            ? "You are running the latest version."
            : state.phase === "available"
              ? `Version ${state.update.version} is available.`
              : state.phase === "downloading"
                ? pct !== null
                  ? `Downloading… ${pct}%`
                  : "Downloading…"
                : state.phase === "ready"
                  ? `Version ${state.update.version} is downloaded and ready to install.`
                  : state.phase === "restarting"
                    ? "Installing and restarting…"
                    : state.phase === "checking"
                      ? "Checking for updates…"
                      : "Checks the latest signed release on GitHub."
        }
      >
        {state.phase === "error" ? (
          <Chip tone="danger">Failed</Chip>
        ) : state.phase === "current" ? (
          <Chip tone="success">Up to date</Chip>
        ) : state.phase === "available" ? (
          <Chip tone="warning">Update available</Chip>
        ) : busy ? (
          <Chip tone="warning">{state.phase === "checking" ? "Checking" : state.phase === "downloading" ? "Downloading" : "Restarting"}</Chip>
        ) : state.phase === "ready" ? (
          <Chip tone="success">Ready</Chip>
        ) : null}
        {state.phase === "available" ? (
          <PrimaryButton onClick={() => void downloadAndInstall()} disabled={busy}>
            Download &amp; install
          </PrimaryButton>
        ) : state.phase === "ready" ? (
          <PrimaryButton onClick={() => void installAndRestart()} disabled={busy}>
            Install &amp; restart
          </PrimaryButton>
        ) : (
          <SecondaryButton onClick={() => void checkForUpdates()} disabled={busy}>
            {state.phase === "checking" ? "Checking…" : "Check for updates"}
          </SecondaryButton>
        )}
      </Row>

      {state.phase === "available" && state.update.body ? (
        <Row label="Release notes" first={false}>
          {/* Rendered as plain text: release body is untrusted input, never HTML. */}
          <div
            style={{
              flex: 1,
              minWidth: 0,
              fontSize: "var(--text-xs)",
              color: "var(--text-secondary)",
              whiteSpace: "pre-wrap",
              wordBreak: "break-word",
              textAlign: "left",
              maxHeight: 120,
              overflowY: "auto",
            }}
          >
            {state.update.body}
          </div>
        </Row>
      ) : null}

      {state.phase === "error" ? (
        <Row label="Last check" first={false}>
          <div
            style={{
              flex: 1,
              minWidth: 0,
              fontSize: "var(--text-xs)",
              color: "var(--danger)",
              whiteSpace: "pre-wrap",
              wordBreak: "break-word",
              textAlign: "left",
            }}
          >
            {state.message}
          </div>
        </Row>
      ) : null}

      <Row
        label="Check on launch"
        hint="Silently check for a new release when Teletype starts. Never downloads or restarts without your say-so."
      >
        <input
          type="checkbox"
          checked={autoCheck}
          onChange={(e) => setAutoCheckPersisted(e.target.checked)}
          aria-label="Check for updates on launch"
        />
      </Row>
      <Row
        label="Releases"
        hint="Browse all published releases and download a build by hand if you prefer."
      >
        <SecondaryButton
          onClick={() => window.open(RELEASES_URL, "_blank", "noopener")}
        >
          Open releases page
        </SecondaryButton>
      </Row>
    </Section>
  );
}

const LICENSE_URL = "https://github.com/alihusains/teletype/blob/main/THIRD-PARTY-NOTICES.txt";

export function LicenseSection() {
  return (
    <Section title="License">
      <Row
        first
        label="MIT License"
        hint="Teletype is open source under the MIT license."
      >
        <SecondaryButton
          onClick={() =>
            window.open("https://github.com/alihusains/teletype", "_blank", "noopener")
          }
        >
          View repository
        </SecondaryButton>
      </Row>
      <Row
        label="Third-party notices"
        hint="The full notices for bundled and linked components ship in THIRD-PARTY-NOTICES.txt in the app bundle."
      >
        <SecondaryButton
          onClick={() => window.open(LICENSE_URL, "_blank", "noopener")}
        >
          View notices
        </SecondaryButton>
      </Row>
    </Section>
  );
}
