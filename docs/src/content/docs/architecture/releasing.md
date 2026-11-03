---
title: "Releasing"
description: "How a tag becomes a universal DMG on GitHub Releases, how the app updates itself, and the secrets a maintainer sets for signing and notarization."
sidebar:
  order: 20
---

A release is a tag. Pushing `v0.1.0` runs `.github/workflows/release.yml`,
which builds Cantina for Apple silicon and Intel in one universal app,
packs it in a DMG, and publishes both to the GitHub release for the tag,
along with the files the in-app updater reads.

## Cutting a release

1. Bump `version` in `src-tauri/tauri.conf.json` and the workspace
   `Cargo.toml` to the new version, and merge that. The workflow refuses
   a tag that does not match `tauri.conf.json`.
2. Optionally write the release first, as a draft or published, with
   notes. The workflow adds its files to a release that exists, and
   otherwise makes one with notes generated from the merged pull
   requests.
3. Tag the merge commit and push the tag:

   ```sh
   git tag v0.1.0 && git push origin v0.1.0
   ```

The workflow takes about fifteen minutes. A pull request that changes the
workflow, `tauri.conf.json`, the entitlements or `Info.plist` runs the
same build without publishing, and keeps the files as workflow artifacts
to try. **Run workflow** on the Actions tab does the same for any branch.

What a release holds:

| File | What it is |
| --- | --- |
| `Cantina_<version>_universal.dmg` | The app, for people to download. |
| `Cantina_<version>_universal.app.tar.gz` | The app, for the updater. |
| `….app.tar.gz.sig` | The updater's signature over the archive. |
| `latest.json` | The update manifest: version, notes, archive URL and signature for both architectures. |

## Updates

The app asks
`https://github.com/jbouder/cantina/releases/latest/download/latest.json`
for the newest release each time it opens (release builds only), and
**Settings → Updates → Check for updates** asks on demand. When a newer
version is out, a banner offers **Install and restart**. It waits for the
project to be saved and for recording to stop, then downloads the
archive, checks its signature against the public key in
`tauri.conf.json` (`plugins.updater.pubkey`), replaces the app and
restarts. An archive whose signature does not verify is refused.
Drafts and pre-releases are never offered, because GitHub's `latest`
skips them.

The updater uses its own key, made with `npx tauri signer generate`, not
Apple's. The private key signs every release from now on: keep it, and
its password, somewhere safe outside the repository. If it is lost,
installed copies can no longer verify updates and have to be updated by
hand with a new DMG that carries a new public key.

## Signing and notarization

macOS checks downloaded apps with Gatekeeper. The workflow signs to the
best level the repository's secrets allow:

| Secrets set | Result |
| --- | --- |
| None | Signed ad hoc. The app runs, but the first open from a download needs **Open Anyway** in System Settings → Privacy & Security. |
| Developer ID certificate | Signed with the certificate. Gatekeeper still asks, because Apple has not checked it. |
| Certificate and Apple ID | Signed and notarized, with the ticket stapled. Opens without a warning. |

The secrets, under **Settings → Secrets and variables → Actions**:

- `APPLE_CERTIFICATE`: a Developer ID Application certificate exported
  from Keychain Access as a `.p12`, base64-encoded
  (`base64 -i cert.p12 | pbcopy`).
- `APPLE_CERTIFICATE_PASSWORD`: the password the `.p12` was exported with.
- `APPLE_SIGNING_IDENTITY`: the certificate's name, as
  `security find-identity -v -p codesigning` prints it, such as
  `Developer ID Application: Your Name (TEAMID)`.
- `APPLE_ID`, `APPLE_PASSWORD` and `APPLE_TEAM_ID`: the Apple ID that
  notarizes, an app-specific password for it from account.apple.com, and
  the team's ID.
- `TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`:
  the updater's private key (the whole key file) and its password.

The app runs under the hardened runtime with the entitlements in
`src-tauri/Entitlements.plist`: audio input, and permission to load
Audio Units and CLAP plugins signed by other developers or not signed at
all, some of which generate code at run time. Notarization accepts these.
`Info.plist` carries the sentence macOS shows when it asks for the
microphone.
