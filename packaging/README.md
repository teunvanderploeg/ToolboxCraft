# Publisher configuration

The release workflow publishes a complete GitHub release when a `v<version>` tag is pushed. It stages assets in a draft, uploads all five update packages plus the signed manifest and checksums, then publishes it as latest. Tags must match the workspace version in Cargo.toml. Manual workflow runs upload reviewable artifacts without publishing. Public tag releases require the Ed25519 update-signing secret. Apple and Windows distribution certificates are optional; automatic updates work without them because manifests and packages are authenticated using the embedded Ed25519 key.

0.1.7 is the first update-enabled build. Users on 0.1.6 or earlier must install it manually once. Future updates install and restart automatically unless disabled in Settings.

## Self-update signing

The repository signing secret is already configured for the public verification key committed in `packaging/update-public-key.hex`. Keep that key stable across releases. For an independent publisher, generate an Ed25519 key with OpenSSL:

```sh
openssl genpkey -algorithm ED25519 -out update-private.pem
openssl pkey -in update-private.pem -pubout -outform DER -out update-public.der
python3 -c 'from pathlib import Path; print(Path("update-public.der").read_bytes()[-32:].hex())'
```

Store the private PEM as the `TOOLBOXCRAFT_UPDATE_PRIVATE_KEY` repository secret and write the printed public key to `packaging/update-public-key.hex`. The repository variable `TOOLBOXCRAFT_UPDATE_PUBLIC_KEY` may also store a visible copy for reference; official builds use the committed public key. Never commit the private key. The release workflow embeds the public key and repository slug, merges all package records, and signs the exact manifest bytes. Changing the trust key requires distributing a launcher build with the new key. The workflow refuses a missing private key, a key that does not match the committed public key, or a missing platform archive. The private key lives only in a temporary directory outside release artifacts while signing.

## macOS

Developer ID signing and notarization are optional for 0.1.7. Without them, macOS packages are ad-hoc signed and initial installation can require Gatekeeper approval. Self-updates still verify the Ed25519 manifest signature and package checksum. Improvements to platform signing are tracked in a separate PR.

When credentials become available, set these repository secrets:

- `MACOS_CERTIFICATE_BASE64`: base64-encoded Developer ID Application P12.
- `MACOS_CERTIFICATE_PASSWORD`: P12 password.
- `MACOS_SIGN_IDENTITY`: signing identity visible to `security find-identity`.
- `MACOS_NOTARY_APPLE_ID`, `MACOS_NOTARY_TEAM_ID`, `MACOS_NOTARY_PASSWORD`: notarization account and app-specific password.

The workflow imports the certificate into a temporary keychain and stores a `ToolboxCraftNotary` notarytool profile when notarization credentials are present. Local packaging also supports an existing keychain identity and `MACOS_NOTARY_PROFILE`. Manifest-authenticated ad-hoc builds can self-update; builds with an Apple signing team retain publisher matching.

## Windows

Windows signing is optional for 0.1.7. Without a trusted certificate, initial installation can show a SmartScreen prompt. Automatic updates still authenticate the publisher through the Ed25519 manifest signature and verify the package checksum. Improvements to Windows distribution signing are tracked in a separate PR.

When credentials become available, set `WINDOWS_CERTIFICATE_BASE64` to the PFX, and `WINDOWS_SIGN_PASSWORD` to its password. The workflow writes it to a temporary file and locates the Windows SDK's signtool. Local packaging can use `WINDOWS_SIGN_PFX` as an existing file path. The existing package script signs executables before constructing archives and signs the NSIS installer after creation.

Windows executables embed the ToolboxCraft artwork and version metadata through `apps/desktop/build.rs`. The installer and uninstaller use the same multi-size ICO. Regenerate it from the bundled PNG with `cargo run -p toolboxcraft --example make_windows_icon`. Packaging verifies both executables contain the expected artwork before creating any Windows archives.

The NSIS installation is per-user and uses `$LOCALAPPDATA/Programs/ToolboxCraft`. Self-updates retain the installer uninstaller executable.

## Linux

The release matrix builds x86_64 on Ubuntu 22.04 and aarch64 on Ubuntu 24.04. ARM releases consequently require a compatible glibc runtime. AppImages carry the launcher and updater helper; install them at a writable user path for self-updates.

Before distributing packages, run native installation, tray, update, running-app deferral, and uninstall smoke tests on macOS, Windows x64/ARM64, and Linux x86_64/ARM64. CI compilation alone cannot confirm desktop integration or signing behavior.
