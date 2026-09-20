import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Icon, type IconName } from "../components/Icon";

type Screen =
  | "home"
  | "dictation"
  | "transforms"
  | "autotext"
  | "personalization"
  | "models"
  | "settings";

interface HistoryEntry {
  id: string;
  createdAt: number;
  text: string;
  appName: string;
  appType: string;
}

const FEATURES: {
  screen: Screen;
  icon: IconName;
  tint: string;
  color: string;
  title: string;
  desc: string;
}[] = [
  {
    screen: "transforms",
    icon: "transforms",
    tint: "#f3e8ff",
    color: "#9333ea",
    title: "Transforms",
    desc: "Polish, rewrite, and make your writing better.",
  },
  {
    screen: "autotext",
    icon: "autotext",
    tint: "#dcfce7",
    color: "#16a34a",
    title: "AutoText",
    desc: "Expand shortcuts instantly.",
  },
  {
    screen: "personalization",
    icon: "personalization",
    tint: "#fef3c7",
    color: "#d97706",
    title: "Personalization",
    desc: "Your style and preferences.",
  },
  {
    screen: "models",
    icon: "models",
    tint: "#e0f2fe",
    color: "#0284c7",
    title: "Models",
    desc: "Choose the best model for your needs.",
  },
];

function greeting(): string {
  const h = new Date().getHours();
  if (h < 12) return "Good morning";
  if (h < 18) return "Good afternoon";
  return "Good evening";
}

export default function HomeScreen({
  onNavigate,
  listening,
}: {
  onNavigate: (s: Screen) => void;
  listening: boolean;
}) {
  const [recent, setRecent] = useState<HistoryEntry[]>([]);

  useEffect(() => {
    invoke<HistoryEntry[]>("list_dictation_history")
      .then((e) => setRecent(e.slice(0, 3)))
      .catch(() => {});
  }, [listening]);

  return (
    <div>
      <h1 style={{ fontSize: 26, fontWeight: 700, letterSpacing: -0.5, marginBottom: 4 }}>
        {greeting()} 👋
      </h1>
      <p style={{ color: "var(--text-secondary)", marginBottom: 20 }}>
        Press your shortcut and start talking, or type / to use AutoText.
      </p>

      {/* Listening / dictation hero card */}
      <div
        style={{
          background: "var(--surface)",
          border: "1px solid var(--border)",
          borderRadius: "var(--radius)",
          padding: 28,
          marginBottom: 20,
          textAlign: "center",
        }}
      >
        {listening ? (
          <>
            <div
              style={{
                display: "inline-flex",
                alignItems: "center",
                gap: 8,
                fontSize: 14,
                color: "var(--text-secondary)",
                marginBottom: 16,
              }}
            >
              <span
                style={{
                  width: 8,
                  height: 8,
                  borderRadius: "50%",
                  background: "var(--accent)",
                  display: "inline-block",
                }}
              />
              Listening…
            </div>
            <HomeWaveform />
            <div style={{ marginTop: 16, fontSize: 13, color: "var(--text-secondary)" }}>
              Release the key to finish
            </div>
          </>
        ) : (
          <>
            <div style={{ fontSize: 15, color: "var(--text-secondary)", marginBottom: 16 }}>
              Press your hotkey to dictate anywhere
            </div>
            <div style={{ display: "flex", justifyContent: "center", gap: 6, marginBottom: 16 }}>
              {Array.from({ length: 40 }).map((_, i) => (
                <span
                  key={i}
                  style={{
                    width: 3,
                    height: 6 + Math.abs(Math.sin(i * 0.5)) * 10,
                    background: "var(--border)",
                    borderRadius: 2,
                    display: "inline-block",
                  }}
                />
              ))}
            </div>
            <div style={{ fontSize: 13, color: "var(--text-secondary)" }}>
              Hold <kbd style={{ background: "var(--surface-2)", padding: "2px 8px", borderRadius: 6, fontSize: 12 }}>Fn</kbd>{" "}
              and speak
            </div>
          </>
        )}
      </div>

      {/* Feature cards */}
      <div
        style={{
          display: "grid",
          gridTemplateColumns: "repeat(auto-fill, minmax(210px, 1fr))",
          gap: 14,
          marginBottom: 24,
        }}
      >
        {FEATURES.map((f) => (
          <button
            key={f.title}
            onClick={() => onNavigate(f.screen)}
            style={{
              textAlign: "left",
              background: "var(--surface)",
              border: "1px solid var(--border)",
              borderRadius: "var(--radius)",
              padding: 16,
              cursor: "pointer",
              display: "flex",
              flexDirection: "column",
              alignItems: "flex-start",
            }}
          >
            <div
              style={{
                width: 38,
                height: 38,
                borderRadius: 10,
                background: f.tint,
                display: "flex",
                alignItems: "center",
                justifyContent: "center",
                marginBottom: 12,
              }}
            >
              <Icon name={f.icon} size={20} color={f.color} />
            </div>
            <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", width: "100%", marginBottom: 4 }}>
              <span style={{ fontWeight: 600 }}>{f.title}</span>
              <Icon name="arrow" size={16} color="var(--text-secondary)" />
            </div>
            <div style={{ fontSize: 13, color: "var(--text-secondary)" }}>{f.desc}</div>
          </button>
        ))}
      </div>

      {/* Recent dictations */}
      {recent.length > 0 && (
        <div>
          <div
            style={{
              fontSize: 12,
              letterSpacing: 1,
              color: "var(--text-secondary)",
              marginBottom: 10,
              display: "flex",
              justifyContent: "space-between",
            }}
          >
            <span>RECENT</span>
            <button onClick={() => onNavigate("dictation")} style={{ padding: "2px 8px" }}>
              View all →
            </button>
          </div>
          <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
            {recent.map((e) => (
              <div
                key={e.id}
                style={{
                  display: "flex",
                  gap: 12,
                  padding: "12px 16px",
                  background: "var(--surface)",
                  border: "1px solid var(--border)",
                  borderRadius: "var(--radius)",
                }}
              >
                <div style={{ flex: 1, minWidth: 0 }}>
                  <p
                    style={{
                      fontSize: 14,
                      overflow: "hidden",
                      textOverflow: "ellipsis",
                      whiteSpace: "nowrap",
                    }}
                  >
                    {e.text}
                  </p>
                  <div style={{ fontSize: 11, color: "var(--text-secondary)", marginTop: 4 }}>
                    {new Date(e.createdAt).toLocaleTimeString("en-US", {
                      hour: "numeric",
                      minute: "2-digit",
                    })}
                    {e.appName ? ` · in ${e.appName}` : ""}
                  </div>
                </div>
              </div>
            ))}
          </div>
        </div>
      )}
    </div>
  );
}

/// A live waveform for the in-app listening card, driven by pill-level events.
function HomeWaveform() {
  const [level, setLevel] = useState(0);
  const [history, setHistory] = useState<number[]>(() => new Array(48).fill(0));

  useEffect(() => {
    let raf = 0;
    let unlisten: (() => void) | undefined;
    import("@tauri-apps/api/event").then(({ listen }) => {
      listen<number>("pill-level", (e) => setLevel(Math.max(0, Math.min(1, e.payload)))).then(
        (fn) => (unlisten = fn)
      );
    });
    const tick = () => {
      setHistory((h) => {
        const next = h.slice(1);
        next.push(level);
        return next;
      });
      setLevel((l) => l * 0.85);
      raf = requestAnimationFrame(tick);
    };
    raf = requestAnimationFrame(tick);
    return () => {
      cancelAnimationFrame(raf);
      unlisten?.();
    };
  }, [level]);

  const maxH = 48;
  return (
    <div style={{ display: "flex", alignItems: "center", justifyContent: "center", gap: 2, height: maxH }}>
      {history.map((v, i) => {
        const h = Math.max(4, Math.min(1, v) * maxH);
        return (
          <span
            key={i}
            style={{
              width: 3,
              height: h,
              borderRadius: 2,
              background: "var(--accent)",
              display: "inline-block",
            }}
          />
        );
      })}
    </div>
  );
}
