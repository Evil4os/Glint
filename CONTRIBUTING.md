# Contributing

This fork is a macOS-only Codex companion. Preserve the minimal surface and existing quota/reset behavior. Do not add settings, menu-bar UI, Dock UI or provider-switching without an explicit product decision.

Run `npm test`, `npm run build` and `cargo test --manifest-path src-tauri/Cargo.toml`. Use mock quota data and never redeem real credits for tests. Validate native positioning and Codex lifecycle separately from browser previews. Do not commit authentication data, local logs, private screenshots or signing credentials. Keep LICENSE and NOTICE in redistributed copies.
