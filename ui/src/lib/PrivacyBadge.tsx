// Small inline-SVG shield used for the on-device privacy claim. No image
// assets, no icon library: just a shield outline with a check, drawn with the
// app's existing design tokens so it reads as part of the product.
export function ShieldIcon({
  size = 16,
  color = "currentColor",
}: {
  size?: number;
  color?: string;
}) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke={color}
      strokeWidth="1.75"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <path d="M12 3 5 6v5c0 4.5 3 7.5 7 9 4-1.5 7-4.5 7-9V6l-7-3z" />
      <path d="m9 12 2 2 4-4" />
    </svg>
  );
}

// A compact "On-device" pill. tone "light" sits on a colored/dark card,
// "default" sits on a white surface.
export function PrivacyBadge({
  label = "On-device",
  tone = "default",
}: {
  label?: string;
  tone?: "default" | "light";
}) {
  const fg = tone === "light" ? "#fff" : "var(--success)";
  const bg =
    tone === "light" ? "rgba(255,255,255,0.14)" : "var(--accent-soft)";
  const border =
    tone === "light" ? "rgba(255,255,255,0.25)" : "rgba(22,163,74,0.25)";
  return (
    <span
      style={{
        display: "inline-flex",
        alignItems: "center",
        gap: 5,
        fontSize: 11,
        fontWeight: 600,
        letterSpacing: 0.3,
        color: fg,
        background: bg,
        border: `1px solid ${border}`,
        borderRadius: 999,
        padding: "3px 10px",
        lineHeight: 1,
        whiteSpace: "nowrap",
      }}
    >
      <ShieldIcon size={12} color={fg} />
      {label}
    </span>
  );
}
