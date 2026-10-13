# Publisher configuration

The release workflow publishes a complete GitHub release when a `v<version>` tag is pushed. It stages assets in a draft, uploads all five update packages plus the signed manifest and checksums, then publishes it as latest. Tags must match the workspace version in Cargo.toml. Manual workflow runs upload reviewable artifacts without publishing. After the platform-signing follow-up is merged and credentials are configured, public tag releases require platform signing; manual test artifacts may remain unsigned. The initial 0.1.7 auto-update release can ship without these certificates.

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

Use an active Apple Developer Program membership to create a **Developer ID Application** certificate (an Apple Development certificate will not work). Export the certificate together with its private key from Keychain Access as a password-protected `.p12`. Add the following values directly to [GitHub Actions secrets](https://github.com/teunvanderploeg/ToolboxCraft/settings/secrets/actions); do not paste private keys or passwords into issues or chat.

Set these repository secrets:

- `MACOS_CERTIFICATE_BASE64`: base64-encoded Developer ID Application P12.
- `MACOS_CERTIFICATE_PASSWORD`: P12 password.
- `MACOS_SIGN_IDENTITY`: signing identity visible to `security find-identity`.
- `MACOS_NOTARY_APPLE_ID`, `MACOS_NOTARY_TEAM_ID`, `MACOS_NOTARY_PASSWORD`: notarization account and app-specific password.

The workflow imports the certificate into a temporary keychain and stores a `ToolboxCraftNotary` notarytool profile when notarization credentials are present. Local packaging also supports an existing keychain identity and `MACOS_NOTARY_PROFILE`. Packaging verifies the signed app, submits it to Apple, requires an Accepted result, staples and validates the ticket, and checks Gatekeeper with `spctl`. It then signs, notarizes, staples, and validates the DMG too. Public releases fail if any step fails. Manifest-authenticated ad-hoc test builds can self-update; builds with an Apple signing team retain publisher matching.

## Windows

Set `WINDOWS_CERTIFICATE_BASE64` to a trusted code-signing PFX, and `WINDOWS_SIGN_PASSWORD` to its password. A self-signed certificate does not establish public trust. For new signing identities, Microsoft recommends [Artifact Signing](https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/code-signing-options); using that service requires configuring its account and adding the service-specific signing integration instead of the PFX path. Many current certificate providers keep private keys in hardware or managed signing services rather than exportable PFX files. The workflow writes it to a temporary file and locates the Windows SDK's signtool. Local packaging can use `WINDOWS_SIGN_PFX` as an existing file path. The package script signs the launcher and updater before constructing archives, signs the NSIS uninstaller through `!uninstfinalize`, and signs the installer after creation. Every signature must pass Windows Authenticode trust validation and include a timestamp. Public releases fail if signing is missing or invalid.

Code signing identifies the publisher but cannot promise zero antivirus detections or immediate SmartScreen trust. [Microsoft documents that new signed files can still prompt until reputation develops](https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/smartscreen-reputation). Microsoft Store distribution is a separate option for reducing installation prompts.

Windows executables embed the ToolboxCraft artwork and version metadata through `apps/desktop/build.rs`. The installer and uninstaller use the same multi-size ICO. Regenerate it from the bundled PNG with `cargo run -p toolboxcraft --example make_windows_icon`. Packaging verifies both executables contain the expected artwork before creating any Windows archives.

The NSIS installation is per-user and uses `$LOCALAPPDATA/Programs/ToolboxCraft`. Self-updates retain the installer uninstaller executable.

## Linux

The release matrix builds x86_64 on Ubuntu 22.04 and aarch64 on Ubuntu 24.04. ARM releases consequently require a compatible glibc runtime. AppImages carry the launcher and updater helper; install them at a writable user path for self-updates.

Before distributing packages, run native installation, tray, update, running-app deferral, and uninstall smoke tests on macOS, Windows x64/ARM64, and Linux x86_64/ARM64. CI compilation alone cannot confirm desktop integration or signing behavior.
