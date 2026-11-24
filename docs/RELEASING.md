# Releasing Caffeinous

There are two builds from the same source:

| Build | Command | Lid-closed mode | Signed with | Ships via |
|---|---|---|---|---|
| Direct | `scripts/bundle.sh --universal` | yes | Developer ID Application | GitHub Releases (dmg), CLI tarball |
| Mac App Store | `scripts/bundle.sh --appstore --universal` | no | Apple Distribution + Mac Installer | App Store Connect (pkg) |

Lid-closed mode runs `pmset -a disablesleep`, which needs root. The App Store sandbox forbids
that, so the App Store build is compiled with `--no-default-features`: no lid menu item, no
password setting, and none of that code in the binary. Apple's review guideline 2.4.5 also
rejects apps that request escalation to root, so do not try to ship the lid feature there.

## GitHub release

1. Bump `[workspace.package] version` in `Cargo.toml`, add a `CHANGELOG.md` section, commit.
2. Make sure CI is green on `main`.
3. Tag and push:

   ```bash
   git tag v0.1.0
   git push origin v0.1.0
   ```

4. The `Release` workflow checks the tag matches the crate version, builds a universal dmg and
   CLI tarball, and attaches them with `SHA256SUMS` to a **draft** release. Review and publish.

### Signing and notarization

Each step runs only when its secrets exist (**Settings → Secrets and variables → Actions**).
Without them the dmg is ad-hoc signed and macOS refuses the first launch; users then have to
open **System Settings → Privacy & Security** and click **Open Anyway**.

**Signing**, needs a Developer ID Application certificate:

| Secret | Value |
|---|---|
| `APPLE_CERTIFICATE` | the certificate and private key exported from Keychain Access as `.p12`, base64: `base64 -i cert.p12 \| pbcopy` |
| `APPLE_CERTIFICATE_PASSWORD` | the password set when exporting the `.p12` |
| `APPLE_SIGNING_IDENTITY` | e.g. `Developer ID Application: Tomas Dewantoro (TEAMID)` |

**Notarization**, either method; Apple ID wins when both are set:

| Secret | Value |
|---|---|
| `APPLE_ID` | Apple ID email |
| `APPLE_PASSWORD` | an app-specific password from <https://account.apple.com> → Sign-In and Security |
| `APPLE_TEAM_ID` | 10-character team ID |
| `APPLE_API_KEY` / `APPLE_API_KEY_ID` / `APPLE_API_ISSUER` | alternatively, an App Store Connect API key (`.p8` contents, key ID, issuer ID) |

These are the same secrets the Slonview repository uses. GitHub never reveals a secret's value,
so they cannot be copied between repositories; set them again from the originals (one Developer
ID certificate can sign any number of apps). The workflow notarizes and staples the dmg and
notarizes the CLI binary separately.

Check a signed release:

```bash
spctl --assess --type execute --verbose /Applications/Caffeinous.app
xcrun stapler validate Caffeinous_0.1.0_universal.dmg
```

## Mac App Store

One-time setup in the Apple Developer account:

1. Register the App ID `io.github.tomasdewantoro.caffeinous` (no extra capabilities).
2. Create the certificates **Apple Distribution** and **Mac Installer Distribution**, and a
   **Mac App Store** provisioning profile for the App ID. Download the profile.
3. In App Store Connect, create the app with that bundle ID. Category: Utilities.

Each submission:

```bash
export SIGN_IDENTITY="Apple Distribution: Tomas Dewantoro (TEAMID)"
export INSTALLER_IDENTITY="3rd Party Mac Developer Installer: Tomas Dewantoro (TEAMID)"
export PROVISIONING_PROFILE=~/Downloads/Caffeinous_Mac_App_Store.provisionprofile
BUILD_NUMBER=$(date +%Y%m%d%H%M) scripts/bundle.sh --appstore --universal
xcrun altool --upload-package dist/Caffeinous-0.1.0.pkg --type macos \
  --apiKey "$APPLE_API_KEY_ID" --apiIssuer "$APPLE_API_ISSUER"
```

`CFBundleVersion` must increase with every upload, hence `BUILD_NUMBER`. Transporter.app also
accepts the pkg if you prefer a GUI.

Review notes worth including: the app holds IOKit power assertions (the same API as
`caffeinate`) and offers Launch at Login through `SMAppService`; it makes no network requests.
