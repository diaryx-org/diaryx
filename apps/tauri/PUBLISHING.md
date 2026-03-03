---
title: Publishing
description: Guide to publishing the Diaryx Tauri app
author: adammharris
audience:
- developers
part_of: '[README](/apps/tauri/README.md)'
---

# Publishing the Diaryx Tauri App

## Prerequisites

### Certificates

Create these at [developer.apple.com/account/resources/certificates](https://developer.apple.com/account/resources/certificates). Each requires a Certificate Signing Request (CSR) generated from Keychain Access → Certificate Assistant → Request a Certificate from a Certificate Authority.

| Certificate | Purpose |
|---|---|
| **Apple Distribution** | Signs `.app` bundles for App Store (iOS + macOS) |
| **Mac Installer Distribution** (or "3rd Party Mac Developer Installer") | Signs `.pkg` for Mac App Store upload |
| **Developer ID Application** | Signs `.app`/`.dmg` for direct distribution (GitHub Releases) |

### App Store Connect API Key

1. Go to [App Store Connect](https://appstoreconnect.apple.com) → Users and Access → Integrations → Team Keys
2. Generate a Team Key with Admin access
3. Note the **Key ID**, **Issuer ID**, and download the `.p8` file
4. Place the `.p8` at `~/.private_keys/AuthKey_<KEY_ID>.p8`

## Publish Scripts

Gitignored scripts that contain signing identities and API keys:

```bash
# iOS — builds, exports, and uploads to App Store Connect
./scripts/publish-ios.sh

# macOS — builds, signs, packages, and uploads to App Store Connect
# Requires a build number argument (must be higher than last upload)
./scripts/publish-macos.sh <build-number>
```

Configuration (API keys, signing identities) is at the top of each script.

## iOS (Manual Steps)

```bash
APPLE_API_KEY=<KEY_ID> \
APPLE_API_ISSUER=<ISSUER_ID> \
APPLE_API_KEY_PATH=~/.private_keys/AuthKey_<KEY_ID>.p8 \
cargo tauri ios build --export-method app-store-connect -- --features iap
```

The IPA is produced at `apps/tauri/src-tauri/gen/apple/build/`.

Upload with:

```bash
xcrun altool --upload-app --type ios \
  --file <path-to-ipa> \
  --apiKey <KEY_ID> \
  --apiIssuer <ISSUER_ID>
```

## macOS App Store (Manual Steps)

### 1. Build

```bash
cargo tauri build --bundles app -- --features iap
```

The `.app` bundle is produced at `target/release/bundle/macos/Diaryx.app`.

### 2. Set build number (must be unique per upload)

```bash
/usr/libexec/PlistBuddy -c "Set :CFBundleVersion <BUILD_NUMBER>" \
  target/release/bundle/macos/Diaryx.app/Contents/Info.plist
```

### 3. Sign the app

```bash
codesign --deep --force --options runtime \
  --sign "Apple Distribution: <YOUR NAME> (<TEAM_ID>)" \
  --entitlements apps/tauri/src-tauri/Entitlements.plist \
  target/release/bundle/macos/Diaryx.app
```

### 4. Package as `.pkg`

```bash
productbuild \
  --component target/release/bundle/macos/Diaryx.app /Applications \
  --sign "3rd Party Mac Developer Installer: <YOUR NAME> (<TEAM_ID>)" \
  Diaryx.pkg
```

### 5. Upload

```bash
xcrun altool --upload-app --type macos \
  --file Diaryx.pkg \
  --apiKey <KEY_ID> \
  --apiIssuer <ISSUER_ID>
```

## macOS (GitHub Releases / Direct Distribution)

Handled by CI in `.github/workflows/tauri-release.yml`. Push a tag matching `v*` to trigger a release build.

For signing and notarization, set these GitHub Secrets:

| Secret | Value |
|---|---|
| `APPLE_CERTIFICATE` | Base64-encoded Developer ID Application `.p12` (`base64 -i cert.p12 \| pbcopy`) |
| `APPLE_CERTIFICATE_PASSWORD` | Password for the `.p12` |
| `APPLE_SIGNING_IDENTITY` | `Developer ID Application: <YOUR NAME> (<TEAM_ID>)` |
| `APPLE_ID` | Apple ID email |
| `APPLE_PASSWORD` | App-specific password (generate at [appleid.apple.com](https://appleid.apple.com)) |
| `APPLE_TEAM_ID` | Your Apple Developer Team ID |

## TestFlight

After uploading a build (iOS or macOS), it appears in [App Store Connect](https://appstoreconnect.apple.com) → TestFlight after processing (5-30 minutes).

- **Internal testing**: Add testers by Apple ID under Internal Testing (up to 100, no review needed)
- **External testing**: Create a group under External Testing, submit for Beta App Review (first time), then enable a public link

## Notes

- iOS icons must not have alpha channels (transparency). If icons are regenerated, flatten them to RGB before building.
- iOS Files app visibility for app `Documents` is enabled with `src-tauri/Info.ios.plist` (`UIFileSharingEnabled` + `LSSupportsOpeningDocumentsInPlace`) wired in `src-tauri/tauri.conf.json` under `bundle.iOS.infoPlist`.
- If you update iOS plist overrides, recreate the iOS project so generated Xcode files pick up the change (`cargo tauri ios init`).
- The `iap` feature flag is required for App Store builds to include the StoreKit 2 plugin.
- Mac App Store builds require sandbox entitlements defined in `src-tauri/Entitlements.plist`.
- The `bundle.category` in `tauri.conf.json` must be set (currently "Productivity") for Mac App Store submission.
