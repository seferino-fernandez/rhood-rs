# Security Policy

## Reporting a vulnerability

Please **do not** open a public issue for security vulnerabilities.

Report privately through GitHub's
[private vulnerability reporting](https://github.com/seferino-fernandez/rhood-rs/security/advisories/new)
(repository **Security** tab → **Report a vulnerability**).

Please include enough detail to reproduce the issue (affected crate, version,
and a minimal example). We aim to acknowledge reports within a few days and
will keep you informed as a fix is developed and released.

## Handling of credentials

`rhood-rs` handles Robinhood account credentials, MFA secrets, and OAuth
tokens. These are read from your local configuration and token cache and are
only ever sent to Robinhood's API — never to any third party or to the
maintainers.

When filing **any** issue (security or otherwise), never paste real
credentials, tokens, MFA secrets, or account identifiers. Redact them first.
