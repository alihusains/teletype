# Signing, notarization, and in-app updates

Teletype ships signed and notarized macOS builds and self-updating installs on
both platforms. This is what has to be true, and what you have to do once.

## Why this exists

An unsigned build makes macOS show a Gatekeeper warning on first launch, and
the user has to right-click the app and choose Open. For a dictation app that
holds microphone and accessibility permissions, that is a bad first impression
and a legitimate reason to uninstall. Signing and notarization remove the
warning; the updater removes the reason to reinstall by hand.

## What is already in the repository

| Piece | Where | Committed? |
|---|---|---|
| Updater plugin wiring | `crates/teletype-desktop/src/lib.rs` | yes |
| Updater public key, endpoint | `crates/teletype-desktop/tauri.conf.json` | yes |
| Updater artifact generation | `tauri.conf.json` `createUpdaterArtifacts` | yes |
| Sign + notarize + publish pipeline | `.github/workflows/release.yml` | yes |
| Updater private key | your machine / CI secret | **no, never** |
| Apple Developer ID certificate | your Apple account / CI secret | **no, never** |

The public key in `tauri.conf.json` is safe to commit. The app needs it to
verify that an update really came from you, and it cannot sign anything.

## One-time setup

### 1. Apple Developer account and certificate

You need a paid Apple Developer Program membership ($99/year) for the
Application Developer ID certificate and for notarization. There is no way
around this for a clean install.

1. Sign in at https://developer.apple.com/account/resources/certificates/list
2. Create a **Developer ID Application** certificate (not a Development one).
   Export it as a `.p12` with a password.
3. Create an **App Store Connect API key** or note your Apple ID and create an
   app-specific password at https://appleid.apple.com.

### 2. Repository secrets

Add these under Settings > Secrets and variables > Actions:

| Secret | Value |
|---|---|
| `APPLE_CERTIFICATE` | the base64 of the `.p12` file |
| `APPLE_CERTIFICATE_PASSWORD` | the password you exported with |
| `APPLE_ID` | your Apple ID email |
| `APPLE_APP_SPECIFIC_PASSWORD` | the app-specific password |
| `APPLE_TEAM_ID` | your 10-character team ID |
| `TAURI_SIGNING_PRIVATE_KEY` | contents of the updater private key |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | its password, if any |

To base64 the certificate without leaving a stray file:

```bash
base64 -i ~/Desktop/DeveloperIDApplication.p12 | pbcopy
```

The workflow decodes it back into a file inside the runner. Never commit a
`.p12` or a private key, including in a private repo.

### 3. The updater keypair

A keypair was generated while setting this up. The **private** key lives
outside the repository and must be copied into the `TAURI_SIGNING_PRIVATE_KEY`
secret. The public key is already in `tauri.conf.json`.

If you need to regenerate (for example the private key was lost), then every
installed copy must be updated manually before the new key can serve updates,
because the old builds verify with the old public key:

```bash
cargo tauri signer generate -w ~/.teletype-updater.key
# then replace plugins.updater.pubkey in tauri.conf.json with the .pub contents
```

**If you lose the updater private key, in-app updates stop working
permanently** for every copy already in the field. There is no recovery and no
key escrow. Put it in a password manager, not just in a CI secret.

## Verifying it worked

```bash
# Local build, signed and notarized
cd ui && npm run build && cd ..
cargo tauri build --bundles app,dmg

# The check users actually hit:
spctl --assess --type execute --verbose \
  target/release/bundle/macos/Teletype.app
# expect: "accepted", "source=Notarized Developer ID"
```

Then push a tag and watch the run:

```bash
git tag v0.1.1 && git push origin v0.1.1
```

In the Release page, confirm all of these are present:

- `Teletype_*.dmg`
- `Teletype_*.app.tar.gz` and its `.sig` (the updater payload)
- `Teletype_*_setup.exe`
- `*.nsis.zip` and its `.sig`
- `latest.json`

`latest.json` must list **both** `darwin-*` and `windows-x86_64`. A manifest
missing a platform means that platform's users are never offered an update,
which is the failure mode that is easiest to ship and hardest to notice.

Finally, install the release build on a clean machine, launch it once, then
check that a later tag is picked up without a manual download.

## How the updater behaves

The Tauri updater plugin is configured with `dialog: true`, so the plugin shows
its own progress and relaunch prompt. There is no custom update UI, which means
there is nothing to keep in sync when the update flow changes.

The app checks the endpoint in `tauri.conf.json` on launch. It does not check
on a timer, so a user who leaves the app open for a week will not be nudged;
they will get the update the next time they launch.

Updates are verified against the public key before anything is written to disk.
An unsigned or tampered payload is rejected, so a compromised release asset
still cannot install code.

## If Apple credentials are missing

The workflow publishes an unsigned build and logs a warning rather than
failing. That is deliberate: an unsigned build that reaches users is more useful
than a signed build that never ships. The consequence is the Gatekeeper
warning, so treat the missing secret as a release blocker for public releases
even though it is not one for CI.
