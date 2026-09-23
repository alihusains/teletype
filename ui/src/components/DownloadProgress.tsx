import { useTauriEvent } from "../lib/useTauriEvent";
import { useState } from "react";

export interface DownloadProgress {
  id: string;
  kind: "llm" | "speech";
  status: "downloading" | "verifying" | "done" | "error";
  fileName: string;
  fileIndex: number;
  fileCount: number;
  fileDownloadedBytes: number;
  fileTotalBytes: number;
  downloadedBytes: number;
  totalBytes: number;
  percent: number;
  speedBps: number;
  etaSeconds: number | null;
  error: string | null;
}

const formatBytes = (n: number) => {
  if (n <= 0) return "0 B";
  if (n >= 1024 * 1024 * 1024) return `${(n / (1024 * 1024 * 1024)).toFixed(2)} GB`;
  if (n >= 1024 * 1024) return `${(n / (1024 * 1024)).toFixed(1)} MB`;
  return `${(n / 1024).toFixed(0)} KB`;
};

const formatSpeed = (bps: number) => {
  if (bps < 1024) return "…";
  return `${formatBytes(bps)}/s`;
};

const formatEta = (secs: number | null) => {
  if (secs == null || secs < 0 || !isFinite(secs)) return "";
  if (secs < 60) return `${Math.ceil(secs)}s left`;
  const m = Math.floor(secs / 60);
  const s = Math.ceil(secs % 60);
  if (m >= 60) {
    const h = Math.floor(m / 60);
    return `${h}h ${m % 60}m left`;
  }
  return `${m}m ${s}s left`;
};

const statusLabel = (p: DownloadProgress) => {
  switch (p.status) {
    case "verifying":
      return "Verifying…";
    case "done":
      return "Done";
    case "error":
      return p.error ?? "Failed";
    default:
      return null;
  }
};

/**
 * Subscribes to `model-download-progress` and renders a bar for `id`.
 * Returns null when this model is not downloading / verifying.
 */
export function useDownloadProgress(id: string): DownloadProgress | null {
  const [progress, setProgress] = useState<DownloadProgress | null>(null);
  useTauriEvent<DownloadProgress>("model-download-progress", (e) => {
    const p = e.payload;
    if (p.id !== id) return;
    if (p.status === "done") {
      setProgress(null);
      return;
    }
    if (p.status === "error") {
      setProgress(p);
      return;
    }
    setProgress(p);
  });
  return progress;
}

export function DownloadProgressBar({ progress }: { progress: DownloadProgress }) {
  const pct = Math.max(0, Math.min(100, progress.percent));
  const multi = progress.fileCount > 1;
  const label = statusLabel(progress);

  return (
    <div style={{ marginTop: 8, width: "100%" }}>
      <div
        style={{
          height: 6,
          borderRadius: 3,
          background: "var(--border)",
          overflow: "hidden",
        }}
      >
        <div
          style={{
            height: "100%",
            width: `${pct}%`,
            borderRadius: 3,
            background:
              progress.status === "error"
                ? "#ef4444"
                : progress.status === "verifying"
                  ? "#f59e0b"
                  : "var(--accent)",
            transition: "width 150ms linear",
          }}
        />
      </div>
      <div
        style={{
          display: "flex",
          justifyContent: "space-between",
          gap: 8,
          marginTop: 4,
          fontSize: 11,
          color:
            progress.status === "error" ? "#ef4444" : "var(--text-secondary)",
          flexWrap: "wrap",
        }}
      >
        <span>
          {label ??
            `${pct.toFixed(1)}% · ${formatBytes(progress.downloadedBytes)}${
              progress.totalBytes > 0 ? ` / ${formatBytes(progress.totalBytes)}` : ""
            }`}
          {multi && progress.fileIndex > 0
            ? ` · file ${progress.fileIndex}/${progress.fileCount}`
            : ""}
          {progress.fileName && multi ? ` (${progress.fileName})` : ""}
        </span>
        <span>
          {progress.status === "downloading"
            ? `${formatSpeed(progress.speedBps)}${
                progress.etaSeconds != null ? ` · ${formatEta(progress.etaSeconds)}` : ""
              }`
            : progress.status === "verifying"
              ? "SHA-256"
              : ""}
        </span>
      </div>
    </div>
  );
}
