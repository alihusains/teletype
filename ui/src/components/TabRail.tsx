import type { ReactNode } from "react";
import { Icon, type IconName } from "./Icon";

export interface TabRailItem {
  id: string;
  label: string;
  tagline: string;
  icon: IconName;
}

/// The fixed left sub-menu card used by the Dictionary page (and reusable for
/// any rail/detail page): one card, one row per tab, each row a 32px icon
/// tile + name + short tagline. The selected row gets the accent fill and a
/// border, the way the reference app's dictionary rail does.
export function TabRail({
  items,
  selected,
  onSelect,
}: {
  items: TabRailItem[];
  selected: string;
  onSelect: (id: string) => void;
}) {
  return (
    <nav
      aria-label="Sections"
      style={{
        display: "flex",
        flexDirection: "column",
        gap: 5,
        padding: 10,
        background: "var(--surface)",
        border: "1px solid var(--border)",
        borderRadius: "var(--radius)",
        width: 216,
        flex: "0 0 216px",
      }}
    >
      {items.map((t) => {
        const isSel = t.id === selected;
        return (
          <button
            key={t.id}
            onClick={() => onSelect(t.id)}
            aria-current={isSel ? "page" : undefined}
            style={{
              display: "flex",
              alignItems: "center",
              gap: 12,
              padding: "9px 10px",
              border: isSel ? "1.5px solid var(--accent)" : "1.5px solid transparent",
              borderRadius: 10,
              background: isSel ? "var(--accent-soft)" : "transparent",
              cursor: "pointer",
              textAlign: "left",
              width: "100%",
            }}
          >
            <span
              style={{
                width: 32,
                height: 32,
                borderRadius: 9,
                flex: "0 0 32px",
                display: "flex",
                alignItems: "center",
                justifyContent: "center",
                background: isSel ? "var(--accent)" : "var(--accent-soft)",
              }}
            >
              <Icon name={t.icon} size={15} color={isSel ? "#fff" : "var(--accent)"} />
            </span>
            <span style={{ minWidth: 0 }}>
              <span
                style={{
                  display: "block",
                  fontSize: 14,
                  fontWeight: 600,
                  color: isSel ? "var(--accent)" : "var(--text)",
                  whiteSpace: "nowrap",
                  overflow: "hidden",
                  textOverflow: "ellipsis",
                }}
              >
                {t.label}
              </span>
              <span
                style={{
                  display: "block",
                  fontSize: 11.5,
                  color: "var(--text-tertiary)",
                  whiteSpace: "nowrap",
                  overflow: "hidden",
                  textOverflow: "ellipsis",
                }}
              >
                {t.tagline}
              </span>
            </span>
          </button>
        );
      })}
    </nav>
  );
}

/// The card that hosts a rail tab's content pane: fills the remaining
/// height, scrolls on its own, so the banner and the rail never move while
/// the pane does.
export function TabPane({ children }: { children: ReactNode }) {
  return (
    <div
      style={{
        flex: 1,
        minWidth: 0,
        background: "var(--surface)",
        border: "1px solid var(--border)",
        borderRadius: "var(--radius)",
        padding: 20,
        overflowY: "auto",
      }}
    >
      {children}
    </div>
  );
}
