# Permission grants arrive after launch

- Hotkey event taps (fn_tap, mod_tap) fail to install without Accessibility. A grant made while the app runs used to change nothing until restart.
- `permission_watch.rs` samples platform permissions (1 s while missing, 5 s once granted), emits `permissions-changed`, and re-registers the hotkey on the main thread on a missing-to-granted transition. Decision logic is the pure `decide`.
- UI: `lib/permissions.ts` (`usePermissions`, launch rule `shouldShowPermissionRepair`) and `OnboardingPermissionsStep`. A user who finished onboarding with a missing permission gets repair mode (permission step plus Try it check).
- Not testable without a real TCC grant on a Mac; verify by hand after any change here.
