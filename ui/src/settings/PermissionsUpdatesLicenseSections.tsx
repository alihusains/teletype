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
      refresh();
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

export function UpdatesSection(props: { version?: string }) {
  const [version, setVersion] = useState<string | null>(props.version ?? null);

  useEffect(() => {
    if (props.version) return;
    getVersion()
      .then(setVersion)
      .catch((e) => console.error("getVersion failed", e));
  }, [props.version]);

  return (
    <Section
      title="Updates"
      hint="This build is not signed with an update key, so it cannot check for or download new versions on its own."
    >
      <Row first label="Version" hint="The version of Teletype running right now.">
        <Chip>{version ?? "unknown"}</Chip>
      </Row>
      <Row
        label="Automatic updates"
        hint="Automatic in-app updates are unavailable in this build. Download a new release by hand when one is published."
      >
        <Chip tone="warning">Unavailable</Chip>
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
