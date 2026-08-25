# Security Policy

## Supported Versions

Nova is pre-release — there is no published version yet. Once there is a
first release, security fixes will target the latest code on `main` until a
stable release line is established.

## Reporting a Vulnerability

Please **do not** open a public GitHub issue for security vulnerabilities.

Instead, report it privately to the maintainers (see the repository's
GitHub Security Advisories tab, or contact a maintainer directly) with:

- A description of the vulnerability and its potential impact.
- Steps to reproduce (proof-of-concept code or commands are helpful).
- The Nova version/commit and platform (OS, whether via the CLI or the
  desktop app) you tested against.

We'll acknowledge your report as soon as we can and follow up with next
steps. Once a fix is available, we'll coordinate on disclosure timing and
credit you in the release notes if you'd like.

## Scope

The areas most relevant to Nova specifically: how the engine resolves
`{{variables}}` and loads environment files (a malicious or untrusted
`nova/` directory shouldn't be able to exfiltrate secrets or execute
arbitrary code just by being opened), and the desktop app's Tauri command
surface (`crates/nova-app/src-tauri/src/commands.rs`) — since those are the
only points where a project on disk crosses into privileged Rust code.
