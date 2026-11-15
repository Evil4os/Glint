# Known limitations

- This edition is macOS-only. The current local share build targets Apple Silicon and macOS 15.2+; Intel and older systems are not part of this release.
- No Developer ID certificate or notarization is included in the local test package. Gatekeeper may block a downloaded copy. An ad-hoc signature is an integrity signature, not a trusted publisher identity.
- An installed app retains a Finder/Spotlight identity even without Dock, menu-bar or settings UI.
- Install under `/Applications` or `~/Applications` and open once to register automatic following. Launching from a disk image or build directory intentionally does not install a watcher.
- The per-user watcher stays running while Codex is closed; the quota main process and its WebViews do not. Normal response is around two seconds, with a 15-second minimum interval between automatic starts after crashes.
- Basic attachment observes window bounds, not screenshots. Optional automatic avoidance of Codex update controls needs separately granted Accessibility access. Startup never prompts for it.
- Quota APIs and the Codex sidebar layout may change independently of this project. Session expiry requires signing in to Codex again. Reset redemption requires a compatible local Codex CLI.
- The transparent halo softens underlying content; it cannot reproduce an empty page background on every desktop surface. Browser preview checks are not a substitute for native desktop acceptance.
