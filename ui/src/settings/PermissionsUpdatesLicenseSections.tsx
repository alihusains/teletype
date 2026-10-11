// Permissions, Updates, and License sections, ported from Envious Wispr's
// PermissionsSettingsView state machine onto Teletype's primitives.
//
// The permission row is the whole point: the action button's label and
// destination depend on the tri-state, because macOS behaves differently per
// state. A permission never requested still prompts on demand, but a refused
// one never prompts again, so a single "Grant Access" button either nags
// forever or sends someone to System Settings who only needed to click Allow.

import { useCallback, useEffect, useState } from "react";
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

// In-app update states. "idle" offers Check for updates; "checking" talks to
// the updater's endpoint (GitHub latest.json on a release tag); "available"
// offers Download; "downloading" streams progress; "ready" offers Restart.
// Errors land in `error` and always return the flow to idle.
type UpdatePhase =
  | { state: "idle" }
  | { state: "checking" }
  | { state: "available"; update: Update; version: string; notes?: string }
  | { state: "downloading"; received: number; total: number | null }
  | { state: "ready" };

export function UpdatesSection(props: { version?: string }) {
  const [version, setVersion] = useState<string | null>(props.version ?? null);
  const [phase, setPhase] = useState<UpdatePhase>({ state: "idle" });
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (props.version) return;
    getVersion()
      .then(setVersion)
      .catch((e) => console.error("getVersion failed", e));
  }, [props.version]);

  const checkForUpdate = async () => {
    setError(null);
    setPhase({ state: "checking" });
    try {
      // `check` fetches the configured endpoint and compares the manifest
      // version against the running one. null here means "already on the
      // latest version".
      const update = await check();
      if (update) {
        setPhase({
          state: "available",
          update,
          version: update.version,
          notes: update.body ?? undefined,
        });
      } else {
        setPhase({ state: "idle" });
      }
    } catch (e) {
      console.error("updater check failed", e);
      setError(String(e));
      setPhase({ state: "idle" });
    }
  };

  const downloadAndInstall = async () => {
    if (phase.state !== "available") return;
    const update = phase.update;
    setError(null);
    setPhase({ state: "downloading", received: 0, total: null });
    try {
      // The progress callback closes over this accumulator rather than the
      // `phase` state, which would be stale between renders.
      let received = 0;
      await update.downloadAndInstall((event) => {
        if (event.event === "Started") {
          received = 0;
          setPhase({ state: "downloading", received: 0, total: event.data.contentLength ?? null });
        } else if (event.event === "Progress") {
          received += event.data.chunkLength;
          setPhase((p) => ({
            state: "downloading",
            received,
            total: p.state === "downloading" ? p.total : null,
          }));
        } else if (event.event === "Finished") {
          setPhase({ state: "ready" });
        }
      });
      // downloadAndInstall resolves after Finished; be explicit in case the
      // event stream ended without a Finished event.
      setPhase((p) => (p.state === "downloading" ? { state: "ready" } : p));
    } catch (e) {
      console.error("updater download failed", e);
      setError(String(e));
      setPhase({ state: "idle" });
    }
  };

  const restart = async () => {
    try {
      await relaunch();
    } catch (e) {
      console.error("relaunch failed", e);
      setError(String(e));
    }
  };

  const busy = phase.state === "checking" || phase.state === "downloading";

  return (
    <Section
      title="Updates"
      hint="Checks the published GitHub release for a newer version; the download is verified against the app's updater signing key before install."
    >
      <Row first label="Version" hint="The version of Teletype running right now.">
        <Chip>{version ?? "unknown"}</Chip>
      </Row>
      <Row
        label={
          phase.state === "available"
            ? "New version available"
            : phase.state === "downloading"
              ? "Downloading update"
              : phase.state === "ready"
                ? "Update ready"
                : "Automatic updates"
        }
        hint={
          phase.state === "idle"
            ? "Teletype checks the GitHub release only when you ask it to — no background phoning home."
            : phase.state === "available"
              ? phase.notes
                ? phase.notes.slice(0, 200)
                : undefined
              : undefined
        }
      >
        {phase.state === "downloading" && (
          <Chip tone="neutral">
            {phase.total != null && phase.total > 0
              ? `${Math.min(100, Math.round((phase.received / phase.total) * 100))}%`
              : `${(phase.received / 1024 / 1024).toFixed(1)} MB`}
          </Chip>
        )}
        {phase.state === "idle" && (
          <PrimaryButton onClick={checkForUpdate} disabled={busy}>
            Check for updates
          </PrimaryButton>
        )}
        {phase.state === "checking" && <Chip tone="neutral">Checking…</Chip>}
        {phase.state === "available" && (
          <>
            <Chip tone="success">v{phase.version}</Chip>
            <PrimaryButton onClick={downloadAndInstall}>Download & install</PrimaryButton>
          </>
        )}
        {phase.state === "downloading" && (
          <SecondaryButton disabled>Downloading…</SecondaryButton>
        )}
        {phase.state === "ready" && (
          <PrimaryButton onClick={restart}>Restart to update</PrimaryButton>
        )}
        <SecondaryButton onClick={() => window.open(RELEASES_URL, "_blank", "noopener")}>
          Open releases page
        </SecondaryButton>
      </Row>
      {error && (
        <Row label="Update failed" hint="The updater could not reach or verify the release. The releases page always has the latest installer.">
          <Chip tone="danger">Error</Chip>
          <SecondaryButton onClick={checkForUpdate}>Try again</SecondaryButton>
        </Row>
      )}
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
