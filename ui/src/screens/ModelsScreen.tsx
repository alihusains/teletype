import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

interface ModelStatus {
  id: string;
  name: string;
  size_mb: number;
  description: string;
  downloaded: boolean;
  selected: boolean;
}

export default function ModelsScreen() {
  const [models, setModels] = useState<ModelStatus[]>([]);
  const [modelStatus, setModelStatus] = useState("");
  const [downloading, setDownloading] = useState<string | null>(null);

  useEffect(() => {
    refresh();
  }, []);

  const refresh = () => {
    invoke<ModelStatus[]>("list_models").then(setModels).catch(console.error);
    invoke<string>("get_model_status").then(setModelStatus).catch(console.error);
  };

  const select = async (id: string) => {
    await invoke("select_model", { id });
    refresh();
  };

  const download = async (id: string) => {
    setDownloading(id);
    try {
      await invoke("download_model", { id });
    } catch (e) {
      alert(`Download failed: ${e}`);
    }
    setDownloading(null);
    refresh();
  };

  return (
    <div>
      <h2 style={{ fontSize: 18, fontWeight: 600, marginBottom: 4 }}>Models</h2>
      <p style={{ color: "var(--text-secondary)", marginBottom: 16 }}>
        Local AI models for text transforms. Downloads are explicit and one-time.
      </p>

      <div style={{ padding: "10px 14px", background: "var(--surface)", borderRadius: "var(--radius)", marginBottom: 16, fontSize: 13 }}>
        <strong>Current model:</strong> {modelStatus || "None"}
      </div>

      <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
        {models.map((m) => (
          <div key={m.id} style={{
            display: "flex", alignItems: "center", gap: 12,
            padding: "12px 16px", background: "var(--surface)",
            borderRadius: "var(--radius)", border: `1px solid ${m.selected ? "var(--accent)" : "var(--border)"}`,
          }}>
            <div style={{ flex: 1 }}>
              <div style={{ fontWeight: 500 }}>{m.name}</div>
              <div style={{ fontSize: 12, color: "var(--text-secondary)" }}>
                {m.description} · ~{Math.round(m.size_mb / 1024 * 10) / 10} GB
              </div>
            </div>
            {m.downloaded ? (
              <button className={m.selected ? "primary" : ""} onClick={() => select(m.id)}>
                {m.selected ? "Selected" : "Select"}
              </button>
            ) : (
              <button onClick={() => download(m.id)} disabled={downloading === m.id}>
                {downloading === m.id ? "Downloading…" : `Download (${Math.round(m.size_mb / 1024 * 10) / 10} GB)`}
              </button>
            )}
          </div>
        ))}
      </div>

      <p style={{ marginTop: 16, fontSize: 12, color: "var(--text-secondary)" }}>
        Models are stored locally and never leave your machine. You can also place any
        GGUF file in the models directory and select it.
      </p>
    </div>
  );
}
