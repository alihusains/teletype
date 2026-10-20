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
    <nav aria-label="Sections" className="tt-tabrail">
      {items.map((t) => {
        const isSel = t.id === selected;
        return (
          <button
            key={t.id}
            onClick={() => onSelect(t.id)}
            aria-current={isSel ? "page" : undefined}
            data-selected={isSel ? "true" : undefined}
            className="tt-tabrail-item"
          >
            <span className="tt-tabrail-icon">
              <Icon name={t.icon} size={15} color={isSel ? "var(--on-accent)" : "var(--accent)"} />
            </span>
            <span style={{ minWidth: 0 }}>
              <span className="tt-tabrail-label">{t.label}</span>
              <span className="tt-tabrail-tagline">{t.tagline}</span>
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
  return <div className="tt-dict-pane">{children}</div>;
}
