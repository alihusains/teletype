import React from "react";

/// A consistent line-icon set (24x24, 1.75px stroke, rounded) used across the
/// app for navigation and actions — no emoji, no external dependency.
export type IconName =
  | "home"
  | "dictation"
  | "transforms"
  | "autotext"
  | "personalization"
  | "models"
  | "dashboard"
  | "insights"
  | "dictionary"
  | "style"
  | "scratchpad"
  | "settings"
  | "copy"
  | "trash"
  | "check"
  | "arrow"
  | "search"
  | "plus"
  | "wave";

const PATHS: Record<IconName, React.ReactNode> = {
  home: (
    <>
      <path d="M3 10.5 12 3l9 7.5" />
      <path d="M5 9.5V21h14V9.5" />
      <path d="M9.5 21v-6h5v6" />
    </>
  ),
  dictation: (
    <>
      <rect x="9" y="3" width="6" height="11" rx="3" />
      <path d="M5 11a7 7 0 0 0 14 0" />
      <path d="M12 18v3" />
    </>
  ),
  transforms: (
    <>
      <path d="M5 5l14 14" />
      <path d="M19 5 5 19" opacity="0" />
      <path d="M15 3l1.5 4L21 8.5 16.5 10 15 14l-1.5-4L9 8.5 13.5 7z" />
      <path d="M6 14l1 2.5L9.5 17 7 18l-1 2.5L5 18l-2.5-1L5 14z" />
    </>
  ),
  autotext: <path d="M13 2 4 14h7l-1 8 9-12h-7z" />,
  personalization: (
    <>
      <circle cx="12" cy="8" r="4" />
      <path d="M4 21c0-4 4-6 8-6s8 2 8 6" />
    </>
  ),
  models: (
    <>
      <rect x="4" y="5" width="16" height="12" rx="3" />
      <path d="M8 21h8" />
      <path d="M12 17v4" />
      <path d="M9 9h.01M15 9h.01" />
    </>
  ),
  dashboard: (
    <>
      <path d="M4 20V10" />
      <path d="M10 20V4" />
      <path d="M16 20v-7" />
      <path d="M22 20H2" />
    </>
  ),
  insights: (
    <>
      <path d="M12 3a6 6 0 0 0-4 10.5c.8.7 1 1.5 1 2.5h6c0-1 .2-1.8 1-2.5A6 6 0 0 0 12 3z" />
      <path d="M9 20h6" />
    </>
  ),
  dictionary: (
    <>
      <path d="M4 19.5A2.5 2.5 0 0 1 6.5 17H20" />
      <path d="M6.5 2H20v20H6.5A2.5 2.5 0 0 1 4 19.5v-15A2.5 2.5 0 0 1 6.5 2z" />
    </>
  ),
  style: (
    <>
      <path d="m12 19 7-7 3 3-7 7-3-3z" />
      <path d="m18 13-1.5-7.5L2 2l3.5 14.5L13 18l5-5z" />
      <path d="m2 2 7.6 7.6" />
      <circle cx="11" cy="11" r="2" />
    </>
  ),
  scratchpad: (
    <>
      <path d="M12 3H5a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-7" />
      <path d="M18.4 2.6a2.1 2.1 0 0 1 3 3L13 14l-4 1 1-4z" />
    </>
  ),
  settings: (
    <>
      <circle cx="12" cy="12" r="3" />
      <path d="M12 2v3M12 19v3M2 12h3M19 12h3M5 5l2 2M17 17l2 2M19 5l-2 2M7 17l-2 2" />
    </>
  ),
  copy: (
    <>
      <rect x="9" y="9" width="12" height="12" rx="2" />
      <path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1" />
    </>
  ),
  trash: (
    <>
      <path d="M3 6h18" />
      <path d="M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2" />
      <path d="M19 6l-1 14a2 2 0 0 1-2 2H8a2 2 0 0 1-2-2L5 6" />
    </>
  ),
  check: <path d="M20 6 9 17l-5-5" />,
  arrow: (
    <>
      <path d="M5 12h14" />
      <path d="M13 6l6 6-6 6" />
    </>
  ),
  search: (
    <>
      <circle cx="11" cy="11" r="7" />
      <path d="M21 21l-4.3-4.3" />
    </>
  ),
  plus: (
    <>
      <path d="M12 5v14M5 12h14" />
    </>
  ),
  wave: (
    <>
      <path d="M4 12v0M8 8v8M12 5v14M16 8v8M20 12v0" />
    </>
  ),
};

export function Icon({
  name,
  size = 20,
  color = "currentColor",
  strokeWidth = 1.75,
  ...rest
}: {
  name: IconName;
  size?: number;
  color?: string;
  strokeWidth?: number;
} & React.SVGProps<SVGSVGElement>) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke={color}
      strokeWidth={strokeWidth}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      {...rest}
    >
      {PATHS[name]}
    </svg>
  );
}
