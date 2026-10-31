import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useTauriEvent } from "./useTauriEvent";

export interface Permission {
  kind: string;
  state: "granted" | "denied" | "notDetermined" | "unsupported";
}

/** How often the UI re-reads permissions while one is still missing. */
export const PERMISSION_POLL_MS = 1000;

/** Permissions the user still has to act on. `unsupported` has nothing to grant. */
export function missingPermissions(perms: Permission[]): Permission[] {
  return perms.filter((p) => p.state !== "granted" && p.state !== "unsupported");
}

/**
 * Whether the permission step must be shown. An empty or not-yet-loaded list
 * is "unknown", never "missing": a slow TCC lookup must not trap a configured
 * user on a setup screen.
 */
export function needsPermissionStep(perms: Permission[] | null): boolean {
  return perms !== null && perms.length > 0 && missingPermissions(perms).length > 0;
}

/**
 * Launch-time rule: a user who finished onboarding earlier still lands on the
 * permission step when a required permission is missing (revoked, or reset by
 * a changed code signature on update). First-run users get the full flow
 * regardless, so this only decides the repair case.
 */
export function shouldShowPermissionRepair(
  onboardingDone: boolean,
  perms: Permission[] | null,
): boolean {
  return onboardingDone && needsPermissionStep(perms);
}

/**
 * Live permission state. Updates from the backend watcher's
 * `permissions-changed` event, and independently re-reads every second while
 * something is missing and whenever the window regains focus (the user is
 * coming back from System Settings), so it never depends on a restart.
 */
export function usePermissions(): { permissions: Permission[] | null; refresh: () => void } {
  const [permissions, setPermissions] = useState<Permission[] | null>(null);

  const refresh = useCallback(() => {
    invoke<Permission[]>("get_permissions")
      .then(setPermissions)
      .catch((e) => console.error("get_permissions failed:", e));
  }, []);

  useEffect(() => {
    refresh();
  }, [refresh]);

  useTauriEvent<Permission[]>("permissions-changed", ({ payload }) => {
    if (Array.isArray(payload)) setPermissions(payload);
  });

  const stillMissing = permissions === null || needsPermissionStep(permissions) || permissions.length === 0;
  useEffect(() => {
    if (!stillMissing) return;
    const id = setInterval(refresh, PERMISSION_POLL_MS);
    return () => clearInterval(id);
  }, [stillMissing, refresh]);

  useEffect(() => {
    window.addEventListener("focus", refresh);
    return () => window.removeEventListener("focus", refresh);
  }, [refresh]);

  return { permissions, refresh };
}
