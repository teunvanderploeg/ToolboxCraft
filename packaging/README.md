# Publisher configuration

The release workflow uploads packages as artifacts for review. Download the `release-assets` artifact and upload its contents to the GitHub release matching the manifest version. Tags must match the workspace version in Cargo.toml; the workflow verifies this.

## Self-update signing

Generate an Ed25519 key with OpenSSL:

```sh
openssl genpkey -algorithm ED25519 -out update-private.pem
openssl pkey -in update-private.pem -pubout -outform DER -out update-public.der
python3 -c 'from pathlib import Path; print(Path("update-public.der").read_bytes()[-32:].hex())'
```

Store the private PEM as the `TOOLBOXCRAFT_UPDATE_PRIVATE_KEY` repository secret and the printed public key as the `TOOLBOXCRAFT_UPDATE_PUBLIC_KEY` repository variable. Never commit the private key. The release workflow embeds the public key and repository slug, merges all package records, and signs the exact manifest bytes. Changing the trust key requires distributing a launcher build with the new key.

## macOS

Set these repository secrets:

- `MACOS_CERTIFICATE_BASE64`: base64-encoded Developer ID Application P12.
- `MACOS_CERTIFICATE_PASSWORD`: P12 password.
- `MACOS_SIGN_IDENTITY`: signing identity visible to `security find-identity`.
- `MACOS_NOTARY_APPLE_ID`, `MACOS_NOTARY_TEAM_ID`, `MACOS_NOTARY_PASSWORD`: notarization account and app-specific password.

The workflow imports the certificate into a temporary keychain and stores a `ToolboxCraftNotary` notarytool profile when notarization credentials are present. Local packaging also supports an existing keychain identity and `MACOS_NOTARY_PROFILE`. macOS self-updates require publisher signing, even though local test DMGs may be ad-hoc signed.

## Windows

Set `WINDOWS_CERTIFICATE_BASE64` to the PFX, and `WINDOWS_SIGN_PASSWORD` to its password. The workflow writes it to a temporary file and locates the Windows SDK's signtool. Local packaging can use `WINDOWS_SIGN_PFX` as an existing file path. The package script signs executables before constructing archives and signs the NSIS installer after creation.

The NSIS installation is per-user and uses `$LOCALAPPDATA/Programs/ToolboxCraft`. Self-updates retain the installer uninstaller executable.

## Linux

The release matrix builds x86_64 on Ubuntu 22.04 and aarch64 on Ubuntu 24.04. ARM releases consequently require a compatible glibc runtime. AppImages carry the launcher and updater helper; install them at a writable user path for self-updates.

Before distributing packages, run native installation, tray, update, running-app deferral, and uninstall smoke tests on macOS, Windows x64/ARM64, and Linux x86_64/ARM64. CI compilation alone cannot confirm desktop integration or signing behavior.
