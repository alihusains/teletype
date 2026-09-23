import { listen } from "@tauri-apps/api/event";

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

const LICENSE_KEY = "teletype.acceptedLicenses";

/**
 * App-wide download state that survives ModelsScreen unmount/remount.
 * App.tsx starts the event listener once; ModelsScreen only subscribes.
 */
const progressById = new Map<string, DownloadProgress>();
const activeIds = new Set<string>();
const listeners = new Set<() => void>();
let storeStarted = false;
let cachedSnapshot: {
  active: string[];
  progress: Record<string, DownloadProgress>;
} | null = null;

function notify() {
  cachedSnapshot = null;
  for (const fn of listeners) fn();
}

export function startDownloadStore() {
  if (storeStarted) return;
  storeStarted = true;
  void listen<DownloadProgress>("model-download-progress", (e) => {
    const p = e.payload;
    if (p.status === "done") {
      progressById.delete(p.id);
      activeIds.delete(p.id);
    } else if (p.status === "error") {
      progressById.set(p.id, p);
      activeIds.delete(p.id);
    } else {
      progressById.set(p.id, p);
      activeIds.add(p.id);
    }
    notify();
  });
}

/** Call when the UI kicks off a download, before the invoke resolves. */
export function markDownloading(id: string) {
  activeIds.add(id);
  notify();
}

/** Clear after invoke settles (error) or when progress says done. */
export function clearDownload(id: string) {
  activeIds.delete(id);
  progressById.delete(id);
  notify();
}

export function isDownloading(id: string): boolean {
  return activeIds.has(id);
}

export function getProgress(id: string): DownloadProgress | undefined {
  return progressById.get(id);
}

export function subscribeDownloads(fn: () => void): () => void {
  listeners.add(fn);
  return () => listeners.delete(fn);
}

export function snapshotDownloads(): {
  active: string[];
  progress: Record<string, DownloadProgress>;
} {
  // Must return a stable reference between mutations or useSyncExternalStore
  // will re-render forever and crash the tree.
  if (!cachedSnapshot) {
    const progress: Record<string, DownloadProgress> = {};
    for (const [k, v] of progressById) progress[k] = v;
    cachedSnapshot = { active: [...activeIds], progress };
  }
  return cachedSnapshot;
}

export function loadAcceptedLicenses(): Record<string, boolean> {
  try {
    const raw = localStorage.getItem(LICENSE_KEY);
    if (!raw) return {};
    const parsed = JSON.parse(raw);
    return parsed && typeof parsed === "object" ? (parsed as Record<string, boolean>) : {};
  } catch {
    return {};
  }
}

export function saveAcceptedLicense(id: string, accepted: boolean) {
  const all = loadAcceptedLicenses();
  if (accepted) all[id] = true;
  else delete all[id];
  try {
    localStorage.setItem(LICENSE_KEY, JSON.stringify(all));
  } catch {
    // ignore quota / private mode
  }
}
