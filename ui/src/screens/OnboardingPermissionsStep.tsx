import { invoke } from "@tauri-apps/api/core";
import { missingPermissions, usePermissions } from "../lib/permissions";

// Detect the platform so the step can show the right guidance.
const isMac = /Mac/i.test(navigator.userAgent);
const isWindows = /Windows/i.test(navigator.userAgent);

const PERMISSION_HINTS: Record<string, { mac: string; win: string }> = {
  microphone: {
    mac: "Teletype needs the microphone to hear you.",
    win: "Teletype needs microphone access to hear you. Turn it on in Settings → Privacy & security → Microphone.",
  },
  accessibility: {
    mac: "Teletype needs Accessibility access to type into whatever app you're using.",
    win: "Windows does not require a separate accessibility grant. If dictation doesn't type into an app, make sure that app is not running as administrator.",
  },
};

/**
 * The permission step. Status is live: it follows the backend watcher and a
 * 1 s re-read while anything is missing, so granting in System Settings flips
 * the row to "Granted" and enables Continue without a restart or a refresh.
 */
export default function OnboardingPermissionsStep({
  onBack,
  onContinue,
}: {
  onBack?: () => void;
  onContinue: () => void;
}) {
  const { permissions, refresh } = usePermissions();
  const list = permissions ?? [];
  const missing = missingPermissions(list);
  const allGranted = list.length > 0 && missing.length === 0;

  return (
    <div>
      <h3 style={{ fontSize: 15, fontWeight: 600, marginBottom: 4 }}>Grant permissions</h3>
      <p style={{ color: "var(--text-secondary)", fontSize: 13, marginBottom: 16 }}>
        {isWindows
          ? "Teletype needs microphone access to hear you. Windows does not require a separate accessibility grant."
          : "Teletype needs the microphone to hear you and accessibility to type into whatever app you're using."}
      </p>
      <div style={{ display: "flex", flexDirection: "column", gap: 10, marginBottom: 20 }}>
        {list.map((p) => {
          const granted = p.state === "granted" || p.state === "unsupported";
          return (
            <div
              key={p.kind}
              data-testid={`permission-${p.kind}`}
              style={{
                display: "flex",
                flexDirection: "column",
                gap: 6,
                padding: "10px 14px",
                background: "var(--bg)",
                borderRadius: "var(--radius-sm)",
                border: `1px solid ${granted ? "var(--success)" : "var(--border)"}`,
              }}
            >
              <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
                <span style={{ flex: 1, textTransform: "capitalize", fontSize: 13 }}>{p.kind}</span>
                {granted ? (
                  <span style={{ color: "var(--success)", fontSize: 13 }}>✓ Granted</span>
                ) : (
                  <>
                    {(p.state !== "denied" || p.kind === "accessibility") && (
                      <button
                        onClick={async () => {
                          await invoke("request_permission", { kind: p.kind }).catch(console.error);
                          [800, 2000, 4000].forEach((ms) => setTimeout(refresh, ms));
                        }}
                      >
                        Grant
                      </button>
                    )}
                    <button
                      className="primary"
                      onClick={() =>
                        invoke("open_permission_settings", { kind: p.kind }).catch(console.error)
                      }
                    >
                      Open System Settings
                    </button>
                  </>
                )}
              </div>
              {PERMISSION_HINTS[p.kind] && (
                <div style={{ fontSize: 11, color: "var(--text-secondary)" }}>
                  {isMac ? PERMISSION_HINTS[p.kind].mac : PERMISSION_HINTS[p.kind].win}
                </div>
              )}
            </div>
          );
        })}
        {list.length === 0 && (
          <div style={{ color: "var(--text-secondary)", fontSize: 13 }}>Loading permissions…</div>
        )}
      </div>
      {!allGranted && list.length > 0 && (
        <p role="status" style={{ color: "var(--text-secondary)", fontSize: 12, marginBottom: 12 }}>
          Turn Teletype on in System Settings. This page updates by itself, no restart needed.
        </p>
      )}
      {allGranted && (
        <p role="status" style={{ color: "var(--success)", fontSize: 12, marginBottom: 12 }}>
          All set. Continue to the next step.
        </p>
      )}
      <div style={{ display: "flex", justifyContent: onBack ? "space-between" : "flex-end" }}>
        {onBack && <button onClick={onBack}>Back</button>}
        <button className="primary" disabled={!allGranted} onClick={onContinue}>
          Continue
        </button>
      </div>
    </div>
  );
}
