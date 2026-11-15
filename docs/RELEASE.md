# macOS release

Build on macOS with Node 22+ and Rust stable:

```sh
npm ci
npm test
cargo test --manifest-path src-tauri/Cargo.toml
npm run tauri -- build --target aarch64-apple-darwin --bundles app
python3 scripts/package-macos.py
python3 scripts/export-source.py
```

The packaging script verifies the app, checks LSUIElement, includes the MIT and font notices, creates a ZIP and SHA-256 file, and verifies the ZIP can be restored with an intact signature. By default it uses an ad-hoc signature and names the archive `-adhoc.zip`. It never copies the development release directory wholesale.

For a signed release, set `APPLE_SIGNING_IDENTITY` during the Tauri build, notarize and staple the built app using your Apple Developer credentials, then run `python3 scripts/package-macos.py --notarized`. This mode requires a Developer ID signature, valid stapled ticket and successful Gatekeeper assessment; it does not silently fall back to ad-hoc signing.

The GitHub workflow produces a **draft test release** on a `v*` tag. Review the artifact and its caveats before publishing. It does not promise signed distribution. No signing credentials belong in the repository.

Install and open the app once. Confirm it registers only `app.quotafloat.desktop.watch`, has no Dock/menu/settings entry, follows Codex launch/exit, preserves reset confirmation, and leaves Codex authentication files unchanged. Test that `--uninstall-watcher` unloads and removes only this user's watcher. Do not consume real reset credits during QA.

Keep LICENSE and NOTICE. Export source using the allowlisted exporter; exclude `.git`, `release`, logs, auth files, environment files, user screenshots and local absolute paths. The upstream origin is not a publishing destination for a personal fork: set your own repository before pushing.
