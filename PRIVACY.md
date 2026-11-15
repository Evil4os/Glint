# Privacy

Glint is a local macOS companion. It has no analytics, advertising, developer-operated collection service, or account setup screen.

The quota process reads the existing Codex authentication file under `CODEX_HOME` or `~/.codex`. It uses the existing access token to query Codex/ChatGPT quota endpoints. Authentication files are read-only: the app does not rewrite them, export them, or send them to the project maintainer. Network errors retain the last valid snapshot as stale; signed-out results clear it.

Reset-credit redemption is only initiated by clicking the count and confirming. It uses a locally installed Codex CLI's App Server protocol. An idempotency key and selected credit identifier are stored locally to support safe retries; uncertain results are not treated as successful resets.

The main widget observes running applications and Codex window bounds for placement. It does not take screenshots or read conversations. If Accessibility permission was granted separately, it checks the role and label of the overlapping update control; it never asks for that permission automatically. Without that permission, the optional update-control avoidance cannot be guaranteed.

A per-user LaunchAgent runs the same executable in `--watch` mode. This path is evaluated before Tauri/WebKit starts. The watcher checks running applications about every two seconds, launches only its installed Glint bundle when Codex is running, and terminates only that bundle's main process when Codex exits. It does not initialize the quota network client or read account files.

The app keeps local lifecycle diagnostics, reset retry state and WebView local storage. Old preference files from earlier editions are left untouched but no longer used. Neither the repository source archive nor release package includes user data, local diagnostics, QA captures, or development backups.

To remove the watcher, run the documented `--uninstall-watcher` command before removing the app. No Codex user data is removed by that command.
