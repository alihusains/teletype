import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

interface RankedItem {
  label: string;
  count: number;
  share: number;
}

interface Insights {
  topPhrases: RankedItem[];
  topApps: RankedItem[];
  avgWordsPerDictation: number;
  busiestHour: number | null;
  vocabularySize: number;
}

const EMPTY: Insights = {
  topPhrases: [],
  topApps: [],
  avgWordsPerDictation: 0,
  busiestHour: null,
  vocabularySize: 0,
};

function ListPanel({ title, items }: { title: string; items: RankedItem[] }) {
  return (
    <div
      style={{
        flex: 1,
        minWidth: 260,
        background: "var(--surface)",
        border: "1px solid var(--border)",
        borderRadius: "var(--radius)",
        padding: "18px 20px",
      }}
    >
      <h3 style={{ fontSize: 13, textTransform: "uppercase", color: "var(--text-secondary)", marginBottom: 14 }}>{title}</h3>
      {items.length === 0 && (
        <p style={{ fontSize: 13, color: "var(--text-secondary)" }}>
          Nothing here yet — dictate a few times and patterns will appear.
        </p>
      )}
      <div style={{ display: "flex", flexDirection: "column", gap: 10 }}>
        {items.map((item) => (
          <div key={item.label}>
            <div style={{ display: "flex", justifyContent: "space-between", fontSize: 13, marginBottom: 3 }}>
              <span style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{item.label}</span>
              <span style={{ color: "var(--text-secondary)", flexShrink: 0, marginLeft: 8 }}>{item.count}×</span>
            </div>
            <div style={{ height: 6, background: "var(--surface-2)", borderRadius: 3 }}>
              <div style={{ height: "100%", width: `${Math.round(item.share * 100)}%`, background: "var(--accent)", borderRadius: 3 }} />
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}

function hourLabel(h: number | null): string {
  if (h === null) return "—";
  const ampm = h < 12 ? "AM" : "PM";
  const hr = h % 12 === 0 ? 12 : h % 12;
  return `${hr} ${ampm}`;
}

export default function InsightsScreen() {
  const [data, setData] = useState<Insights>(EMPTY);

  const refresh = useCallback(() => {
    invoke<Insights>("get_insights").then(setData).catch(console.error);
  }, []);

  useEffect(refresh, [refresh]);

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 18, maxWidth: 900 }}>
      <div>
        <h2 style={{ fontSize: 20, fontWeight: 700 }}>Insights</h2>
        <p style={{ color: "var(--text-secondary)", fontSize: 13 }}>
          What you say most, and where — computed locally from your dictation history.
        </p>
      </div>

      <div style={{ display: "flex", gap: 14, flexWrap: "wrap" }}>
        <ListPanel title="Top phrases" items={data.topPhrases} />
        <ListPanel title="Where you dictate" items={data.topApps} />
      </div>

      <div style={{ display: "flex", gap: 14, flexWrap: "wrap" }}>
        {[
          ["Avg words / dictation", data.avgWordsPerDictation.toLocaleString()],
          ["Busiest hour", hourLabel(data.busiestHour)],
          ["Vocabulary size", data.vocabularySize.toLocaleString()],
        ].map(([label, value]) => (
          <div
            key={label}
            style={{
              background: "var(--surface)",
              border: "1px solid var(--border)",
              borderRadius: "var(--radius)",
              padding: "14px 20px",
              minWidth: 150,
            }}
          >
            <div style={{ fontSize: 12, color: "var(--text-secondary)", textTransform: "uppercase", letterSpacing: 0.4 }}>{label}</div>
            <div style={{ fontSize: 22, fontWeight: 700, marginTop: 2 }}>{value}</div>
          </div>
        ))}
      </div>
    </div>
  );
}
