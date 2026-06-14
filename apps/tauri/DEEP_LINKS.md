# Deep links & Universal Links

Diaryx supports opening into specific actions via links. Two mechanisms share a
single in-app router:

- **Universal Links** (iOS / macOS) and **App Links** (Android) — real
  `https://app.diaryx.org/...` URLs that open the app when installed and fall
  back to the website otherwise.
- **`diaryx://` custom scheme** — desktop fallback (Windows / Linux), where the
  OS has no universal-link equivalent.

## Supported actions

| URL | Action |
| --- | --- |
| `https://app.diaryx.org/open?path=<path>` | Open an existing entry |
| `https://app.diaryx.org/new` | Open the "new entry" modal |
| `https://app.diaryx.org/new?title=<title>&parent=<path>` | Create an entry directly (parent optional) |
| `https://app.diaryx.org/search?q=<query>` | Open the command palette |

The `diaryx://` equivalents use the action as the host, e.g.
`diaryx://open?path=2026/06/14.md`.

> `path` is the workspace path as the app refers to entries internally (the same
> value used by in-app links and search results).

## How it's wired

| Concern | Location |
| --- | --- |
| Plugin registration | `src-tauri/src/lib.rs` (`tauri_plugin_deep_link::init()` + desktop `register_all()`) |
| Scheme / associated-domain config | `src-tauri/tauri.conf.json` → `plugins.deep-link` |
| Capability grant | `src-tauri/capabilities/{default,mobile}.json` → `deep-link:default` |
| macOS associated domain | `src-tauri/Entitlements.plist` |
| In-app routing | `apps/web/src/controllers/deepLinkController.ts`, wired in `App.svelte` (`onMount`/`onDestroy`) |
| Apple association file | `apps/web/public/.well-known/apple-app-site-association` |
| Android association file | `apps/web/public/.well-known/assetlinks.json` |
| Association file headers | `apps/web/public/_headers` |

The two `.well-known` files are served by the same Cloudflare deployment that
hosts `app.diaryx.org` (`apps/web` → `dist`).

## Before shipping — required setup

### 1. Apple (iOS + macOS)

1. Replace `TEAMID` in `apps/web/public/.well-known/apple-app-site-association`
   with the real Apple Developer **Team ID** (the prefix is `TeamID.BundleID`,
   i.e. `<TEAMID>.org.diaryx.desktop`). The Team ID is available in the Apple
   Developer portal and is already stored in CI secrets / on the maintainer's
   machine.
2. The iOS associated-domains entitlement is generated from
   `tauri.conf.json` → `plugins.deep-link.mobile` during `tauri ios build`, so
   no committed iOS entitlements file is needed (the `gen/` dir is regenerated).
   The macOS entitlement is committed in `Entitlements.plist`.
3. Deploy `app.diaryx.org` so the association file is live at
   `https://app.diaryx.org/.well-known/apple-app-site-association`
   (HTTPS, `Content-Type: application/json`, **no redirects**).

### 2. Android (placeholder for now)

Replace `REPLACE_WITH_ANDROID_SIGNING_CERT_SHA256_FINGERPRINT` in
`apps/web/public/.well-known/assetlinks.json` with the SHA-256 fingerprint of
the app's signing certificate:

```sh
keytool -list -v -keystore <release.keystore> -alias <alias>
```

Until then, Android App Link auto-verification will fail (links open the
browser), which is the intended placeholder behaviour.

## Testing

- **iOS / macOS:** paste a `https://app.diaryx.org/open?path=...` link into Notes
  or Messages and tap it on a device with the app installed. Validate the
  association file with Apple's
  [AASA validator](https://branch.io/resources/aasa-validator/). Apple caches
  AASA aggressively; reinstalling the app forces a refresh.
- **Android:** `adb shell am start -a android.intent.action.VIEW -d "https://app.diaryx.org/open?path=test.md"`
  and verify auto-verification with `adb shell pm get-app-links org.diaryx.desktop`.
- **Desktop (`diaryx://`):**
  - macOS: `open "diaryx://open?path=test.md"`
  - Linux: `xdg-open "diaryx://open?path=test.md"`
  - Windows: `start "" "diaryx://open?path=test.md"`

## Known limitations / follow-ups

- **Desktop Windows/Linux single instance:** a `diaryx://` link launches a new
  process. To forward links into an already-running window, add
  `tauri-plugin-single-instance` and have its handler call into the deep-link
  plugin. Not required for the universal-links (Apple/Android) path.
- **`search` query pre-fill:** `search?q=` currently just opens the command
  palette; seeding the query awaits a dedicated content-search surface.
- **`today` action:** not implemented yet — would need a workspace convention
  for locating/creating the current day's entry.
- **Web fallback:** since these are real URLs, `app.diaryx.org/open?path=...`
  should ideally render something sensible for users without the app installed.
