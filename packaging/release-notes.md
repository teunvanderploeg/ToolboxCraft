ToolboxCraft 0.1.7 enables automatic updates for ToolboxCraft itself.

- Automatic ToolboxCraft updates are enabled by default, including when upgrading existing settings.
- The launcher checks at startup and every six hours, verifies the signed release manifest and package checksum, and installs updates after app operations finish. It then restarts automatically.
- Settings includes a separate ToolboxCraft auto-update switch, update status, a manual check, and a retry button.
- Ad-hoc signed macOS builds can update using the pinned release signing key. Builds signed with an Apple publisher certificate retain publisher matching.
- Tagged releases now publish complete, signed update files automatically. Missing or mismatched signing keys stop the release.
- Published macOS downloads require Developer ID signing, Apple notarization, stapled tickets, and Gatekeeper validation. Windows executables, updater, installer, and uninstaller require trusted, timestamped signatures.

**Install 0.1.7 manually once if you are running 0.1.6 or earlier.** Those versions were built without update configuration. Future versions will update automatically while ToolboxCraft is running.

Download the matching DMG, Windows setup EXE or portable ZIP, or Linux AppImage from the assets below. On macOS, quit ToolboxCraft and replace the copy in Applications. Windows SmartScreen can still prompt for a new signed release until publisher and file reputation builds.
