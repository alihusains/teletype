import { invoke } from "@tauri-apps/api/core";

interface CrashNoticeProps {
  message: string;
  onDismiss: () => void;
}

export default function CrashNotice({ message, onDismiss }: CrashNoticeProps) {
  const openDetails = async () => {
    try {
      await invoke("open_crash_dir");
    } catch (e) {
      console.warn("[teletype] open_crash_dir failed:", e);
    }
  };

  return (
    <div
      role="status"
      style={{
        display: "flex",
        alignItems: "flex-start",
        gap: 12,
        marginBottom: 18,
        padding: "12px 16px",
        borderRadius: "var(--radius)",
        background: "var(--warning-soft)",
        border: "1px solid color-mix(in srgb, var(--warning) 30%, transparent)",
        fontSize: 13,
        lineHeight: 1.5,
      }}
    >
      <span style={{ color: "var(--warning)", display: "grid", placeItems: "center", flex: "0 0 auto" }}>
        <svg width="16" height="16" viewBox="0 0 16 16" fill="none" aria-hidden>
          <path
            d="M8 1L15 14H1L8 1Z"
            stroke="currentColor"
            strokeWidth="1.5"
            strokeLinejoin="round"
          />
          <path d="M8 6V10" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" />
          <circle cx="8" cy="12" r="0.75" fill="currentColor" />
        </svg>
      </span>
      <span style={{ flex: 1, minWidth: 0 }}>{message}</span>
      <div style={{ display: "flex", gap: 8, flex: "0 0 auto" }}>
        <button
          onClick={openDetails}
          style={{
            padding: "4px 10px",
            fontSize: 12,
            fontWeight: 600,
            color: "var(--text)",
            background: "transparent",
            border: "1px solid var(--border)",
            borderRadius: "var(--radius-sm)",
            cursor: "pointer",
          }}
        >
          Details
        </button>
        <button
          onClick={onDismiss}
          style={{
            padding: "4px 10px",
            fontSize: 12,
            fontWeight: 600,
            color: "var(--text)",
            background: "transparent",
            border: "1px solid var(--border)",
            borderRadius: "var(--radius-sm)",
            cursor: "pointer",
          }}
        >
          Dismiss
        </button>
      </div>
    </div>
  );
}
