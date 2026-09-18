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

interface SpeechModelStatus {
  id: string;
  name: string;
  engine: string;
  size_mb: number;
  description: string;
  recommended: boolean;
  english_only: boolean;
  downloaded: boolean;
  selected: boolean;
}

const sizeLabel = (mb: number) =>
  mb >= 1024 ? `${(mb / 1024).toFixed(1)} GB` : `${mb} MB`;

export default function ModelsScreen() {
  const [models, setModels] = useState<ModelStatus[]>([]);
  const [speechModels, setSpeechModels] = useState<SpeechModelStatus[]>([]);
  const [modelStatus, setModelStatus] = useState("");
  const [downloading, setDownloading] = useState<string | null>(null);

  useEffect(() => {
    refresh();
  }, []);

  const refresh = () => {
    invoke<ModelStatus[]>("list_models").then(setModels).catch(console.error);
    invoke<SpeechModelStatus[]>("list_speech_models").then(setSpeechModels).catch(console.error);
    invoke<string>("get_model_status").then(setModelStatus).catch(console.error);
  };

  const select = async (id: string) => {
    await invoke("select_model", { id });
    refresh();
  };

  const selectSpeech = async (id: string) => {
    await invoke("select_speech_model", { id });
    refresh();
  };

  const download = async (id: string, speech = false) => {
    setDownloading(id);
    try {
      await invoke(speech ? "download_speech_model" : "download_model", { id });
    } catch (e) {
      alert(`Download failed: ${e}`);
    }
    setDownloading(null);
    refresh();
  };

  const SpeechRow = ({ m }: { m: SpeechModelStatus }) => (
    <div style={{
      display: "flex", alignItems: "center", gap: 12,
      padding: "12px 16px", background: "var(--surface)",
      borderRadius: "var(--radius)", border: `1px solid ${m.selected ? "var(--accent)" : "var(--border)"}`,
    }}>
      <div style={{ flex: 1 }}>
        <div style={{ fontWeight: 500 }}>
          {m.name}
          {m.recommended && (
            <span style={{
              marginLeft: 8, fontSize: 11, fontWeight: 500,
              color: "var(--accent)", background: "var(--surface-2, rgba(0,0,0,0.05))",
              padding: "1px 6px", borderRadius: 4,
            }}>Recommended</span>
          )}
          {m.english_only && (
            <span style={{ marginLeft: 8, fontSize: 11, color: "var(--text-secondary)" }}>English</span>
          )}
          <span style={{
            marginLeft: 8, fontSize: 11,
            color: "var(--text-secondary)",
            border: "1px solid var(--border)",
            padding: "1px 5px", borderRadius: 4,
          }}>{m.engine === "parakeet" ? "Parakeet" : "Whisper"}</span>
        </div>
        <div style={{ fontSize: 12, color: "var(--text-secondary)" }}>
          {m.description} · {sizeLabel(m.size_mb)}
        </div>
      </div>
      {m.downloaded ? (
        <button className={m.selected ? "primary" : ""} onClick={() => selectSpeech(m.id)}>
          {m.selected ? "Selected" : "Select"}
        </button>
      ) : (
        <button onClick={() => download(m.id, true)} disabled={downloading === m.id}>
          {downloading === m.id ? "Downloading…" : `Download (${sizeLabel(m.size_mb)})`}
        </button>
      )}
    </div>
  );

  return (
    <div>
      <h2 style={{ fontSize: 18, fontWeight: 600, marginBottom: 4 }}>Models</h2>
      <p style={{ color: "var(--text-secondary)", marginBottom: 16 }}>
        Local AI models. Downloads are explicit and one-time; models never leave your machine.
      </p>

      <h3 style={{ fontSize: 14, fontWeight: 600, margin: "20px 0 4px" }}>Dictation (speech)</h3>
      <p style={{ fontSize: 12, color: "var(--text-secondary)", marginBottom: 10 }}>
        Transcribes what you say. Parakeet TDT v3 is the fastest and most accurate for English.
      </p>
      <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
        {speechModels.map((m) => <SpeechRow key={m.id} m={m} />)}
      </div>

      <h3 style={{ fontSize: 14, fontWeight: 600, margin: "24px 0 4px" }}>Transforms (LLM)</h3>
      <p style={{ fontSize: 12, color: "var(--text-secondary)", marginBottom: 10 }}>
        Rewrites and polishes your dictated text.
      </p>
      <div style={{ padding: "10px 14px", background: "var(--surface)", borderRadius: "var(--radius)", marginBottom: 12, fontSize: 13 }}>
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
                {m.description} · {sizeLabel(m.size_mb)}
              </div>
            </div>
            {m.downloaded ? (
              <button className={m.selected ? "primary" : ""} onClick={() => select(m.id)}>
                {m.selected ? "Selected" : "Select"}
              </button>
            ) : (
              <button onClick={() => download(m.id)} disabled={downloading === m.id}>
                {downloading === m.id ? "Downloading…" : `Download (${sizeLabel(m.size_mb)})`}
              </button>
            )}
          </div>
        ))}
      </div>

      <p style={{ marginTop: 16, fontSize: 12, color: "var(--text-secondary)" }}>
        You can also place any GGUF file in the models directory for transforms.
      </p>
    </div>
  );
}
