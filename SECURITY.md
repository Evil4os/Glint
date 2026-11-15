# Security

## Supported Use

Glint is a local desktop utility that reads Codex quota using the user's existing Codex Desktop login state.

## Reporting Issues

Please do not open public issues containing tokens, account IDs, raw backend responses, screenshots with personal data, or local file paths. Redact sensitive information before sharing logs or bug reports.

## Security Boundaries

- The app does not persist Codex credentials.
- The app does not log request headers or raw quota responses.
- The app caps auth file reads at 256 KB and quota responses at 1 MB.
- The app does not follow redirects for quota HTTP requests.
- Reset credits are redeemed only after explicit in-app confirmation; uncertain attempts retain their idempotency key. No account settings are changed.

## Release Notes For Maintainers

Before publishing a release, verify:

- Source archives do not include local installers, build outputs, `.codex`, QA screenshots, or environment files.
- macOS bundles are built by CI or a clean machine.
- Ad-hoc test builds are labeled as not notarized; they are not presented as trusted Developer ID releases.
- Signed releases are produced only with maintainer-controlled certificates and secrets.
