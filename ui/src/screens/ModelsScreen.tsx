import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Icon } from "../components/Icon";

interface ModelStatus {
  id: string;
  name: string;
  sizeMb: number;
  description: string;
  downloaded: boolean;
  selected: boolean;
}

interface SpeechModelStatus {
  id: string;
  name: string;
  engine: string;
  sizeMb: number;
  description: string;
  recommended: boolean;
  englishOnly: boolean;
  downloaded: boolean;
  selected: boolean;
  speed: number;
  accuracy: number;
  minRamGb: number;
  languageLabel: string;
}

const sizeLabel = (mb: number) =>
  mb >= 1024 ? `${(mb / 1024).toFixed(1)} GB` : `${mb} MB`;

/// Renders a row of small bars (like SpeakType's speed/accuracy indicators).
function Bars({ value, max = 10 }: { value: number; max?: number }) {
  const filled = Math.round((value / max) * 8);
  return (
    <div style={{ display: "flex", gap: 2, alignItems: "flex-end" }}>
      {Array.from({ length: 8 }, (_, i) => (
        <div
          key={i}
          style={{
            width: 4,
            height: 4 + i * 1.5,
            borderRadius: 1,
            background: i < filled ? "var(--accent)" : "var(--border)",
          }}
        />
      ))}
    </div>
  );
}

function speedLabel(speed: number): string {
  if (speed >= 9.5) return "Blazing";
  if (speed >= 8.5) return "Very fast";
  if (speed >= 7.5) return "Fast";
  if (speed >= 6.0) return "Moderate";
  return "Slow";
}

function accuracyLabel(accuracy: number): string {
  if (accuracy >= 9.5) return "Flawless";
  if (accuracy >= 9.0) return "Sharp";
  if (accuracy >= 8.0) return "Solid";
  if (accuracy >= 6.5) return "Fair";
  return "Basic";
}

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

  const onThisComputer = speechModels.filter((m) => m.downloaded);
  const available = speechModels.filter((m) => !m.downloaded);

  const SpeechRow = ({ m, showDownload }: { m: SpeechModelStatus; showDownload: boolean }) => (
    <div
      style={{
        display: "flex",
        alignItems: "center",
        gap: 16,
        padding: "14px 16px",
        borderBottom: "1px solid var(--border)",
        background: m.selected ? "var(--accent-soft)" : "transparent",
      }}
    >
      {/* Radio / check */}
      <div
        style={{
          width: 20,
          height: 20,
          borderRadius: "50%",
          border: `2px solid ${m.selected ? "var(--accent)" : "var(--border)"}`,
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          flexShrink: 0,
        }}
      >
        {m.selected && (
          <div style={{ width: 10, height: 10, borderRadius: "50%", background: "var(--accent)" }} />
        )}
      </div>

      {/* Name + description */}
      <div style={{ flex: 1, minWidth: 0 }}>
        <div style={{ display: "flex", alignItems: "center", gap: 8, flexWrap: "wrap" }}>
          <span style={{ fontWeight: 600, fontSize: 14 }}>{m.name}</span>
          {m.recommended && (
            <span
              style={{
                fontSize: 11,
                fontWeight: 600,
                color: "var(--success, #22c55e)",
                background: "var(--success-soft, #dcfce7)",
                padding: "1px 8px",
                borderRadius: 10,
              }}
            >
              Recommended
            </span>
          )}
          {m.selected && (
            <span
              style={{
                fontSize: 11,
                fontWeight: 600,
                color: "var(--accent)",
                display: "flex",
                alignItems: "center",
                gap: 3,
              }}
            >
              <Icon name="check" size={12} /> In use
            </span>
          )}
        </div>
        <div style={{ fontSize: 12, color: "var(--text-secondary)", marginTop: 2 }}>
          {m.description}
        </div>
        <div style={{ fontSize: 11, color: "var(--text-secondary)", marginTop: 2, display: "flex", gap: 8 }}>
          <span>{m.languageLabel}</span>
          {m.engine === "whisper" && <span>· Neural Engine</span>}
        </div>
      </div>

      {/* Speed */}
      <div style={{ width: 90, flexShrink: 0 }}>
        <div style={{ fontSize: 11, color: "var(--text-secondary)", marginBottom: 4 }}>
          {speedLabel(m.speed)}
        </div>
        <Bars value={m.speed} />
      </div>

      {/* Accuracy */}
      <div style={{ width: 90, flexShrink: 0 }}>
        <div style={{ fontSize: 11, color: "var(--text-secondary)", marginBottom: 4 }}>
          {accuracyLabel(m.accuracy)}
        </div>
        <Bars value={m.accuracy} />
      </div>

      {/* Size */}
      <div style={{ width: 60, flexShrink: 0, fontSize: 13, color: "var(--text-secondary)" }}>
        {sizeLabel(m.sizeMb)}
      </div>

      {/* Action */}
      <div style={{ width: 80, flexShrink: 0, textAlign: "right" }}>
        {showDownload ? (
          <button
            onClick={() => download(m.id, true)}
            disabled={downloading === m.id}
            style={{ fontSize: 13 }}
          >
            {downloading === m.id ? "Downloading…" : "Download"}
          </button>
        ) : (
          <button
            className={m.selected ? "primary" : ""}
            onClick={() => selectSpeech(m.id)}
            style={{ fontSize: 13 }}
          >
            {m.selected ? "Selected" : "Select"}
          </button>
        )}
      </div>
    </div>
  );

  const tableHeader = (
    <div
      style={{
        display: "flex",
        alignItems: "center",
        gap: 16,
        padding: "8px 16px",
        fontSize: 11,
        fontWeight: 600,
        color: "var(--text-secondary)",
        textTransform: "uppercase",
        letterSpacing: 0.5,
        borderBottom: "1px solid var(--border)",
      }}
    >
      <div style={{ width: 20 }} />
      <div style={{ flex: 1 }}>Model</div>
      <div style={{ width: 90 }}>Speed</div>
      <div style={{ width: 90 }}>Accuracy</div>
      <div style={{ width: 60 }}>Size</div>
      <div style={{ width: 80 }} />
    </div>
  );

  return (
    <div style={{ maxWidth: 860 }}>
      <div style={{ display: "flex", justifyContent: "space-between", alignItems: "flex-start" }}>
        <div>
          <h2 style={{ fontSize: 20, fontWeight: 700 }}>AI Models</h2>
          <p style={{ color: "var(--text-secondary)", fontSize: 13, marginTop: 4 }}>
            Every model runs on this computer. Pick the one Teletype uses to transcribe.
          </p>
        </div>
      </div>

      {/* On this computer */}
      <h3 style={{ fontSize: 15, fontWeight: 700, margin: "24px 0 2px" }}>On this computer</h3>
      <p style={{ fontSize: 12, color: "var(--text-secondary)", marginBottom: 8 }}>
        Click a model to use it for dictation.
      </p>
      <div
        style={{
          background: "var(--surface)",
          border: "1px solid var(--border)",
          borderRadius: "var(--radius)",
          overflow: "hidden",
        }}
      >
        {onThisComputer.length === 0 ? (
          <p style={{ padding: 24, fontSize: 13, color: "var(--text-secondary)", textAlign: "center" }}>
            No models downloaded yet. Grab one from below.
          </p>
        ) : (
          <>
            {tableHeader}
            {onThisComputer.map((m) => (
              <SpeechRow key={m.id} m={m} showDownload={false} />
            ))}
          </>
        )}
      </div>

      {/* Available to download */}
      <h3 style={{ fontSize: 15, fontWeight: 700, margin: "24px 0 2px" }}>Available to download</h3>
      <p style={{ fontSize: 12, color: "var(--text-secondary)", marginBottom: 8 }}>
        Larger models are more accurate. Smaller ones are faster.
      </p>
      <div
        style={{
          background: "var(--surface)",
          border: "1px solid var(--border)",
          borderRadius: "var(--radius)",
          overflow: "hidden",
        }}
      >
        {available.length === 0 ? (
          <p style={{ padding: 24, fontSize: 13, color: "var(--text-secondary)", textAlign: "center" }}>
            All models are downloaded.
          </p>
        ) : (
          <>
            {tableHeader}
            {available.map((m) => (
              <SpeechRow key={m.id} m={m} showDownload={true} />
            ))}
          </>
        )}
      </div>

      {/* Transforms (LLM) */}
      <h3 style={{ fontSize: 15, fontWeight: 700, margin: "28px 0 2px" }}>Transforms (LLM)</h3>
      <p style={{ fontSize: 12, color: "var(--text-secondary)", marginBottom: 8 }}>
        Rewrites and polishes your dictated text.
      </p>
      <div style={{ padding: "10px 14px", background: "var(--surface)", borderRadius: "var(--radius)", marginBottom: 8, fontSize: 13 }}>
        <strong>Current model:</strong> {modelStatus || "None"}
      </div>
      <div style={{ display: "flex", flexDirection: "column", gap: 4 }}>
        {models.map((m) => (
          <div
            key={m.id}
            style={{
              display: "flex",
              alignItems: "center",
              gap: 12,
              padding: "10px 16px",
              background: "var(--surface)",
              borderRadius: "var(--radius-sm)",
              border: `1px solid ${m.selected ? "var(--accent)" : "var(--border)"}`,
            }}
          >
            <div style={{ flex: 1 }}>
              <div style={{ fontWeight: 500, fontSize: 13 }}>{m.name}</div>
              <div style={{ fontSize: 12, color: "var(--text-secondary)" }}>
                {m.description} · {sizeLabel(m.sizeMb)}
              </div>
            </div>
            {m.downloaded ? (
              <button className={m.selected ? "primary" : ""} onClick={() => invoke("select_model", { id: m.id }).then(refresh)}>
                {m.selected ? "Selected" : "Select"}
              </button>
            ) : (
              <button onClick={() => download(m.id)} disabled={downloading === m.id}>
                {downloading === m.id ? "Downloading…" : `Download (${sizeLabel(m.sizeMb)})`}
              </button>
            )}
          </div>
        ))}
      </div>
    </div>
  );
}
