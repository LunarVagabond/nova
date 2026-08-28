# Documentation

**[README.md](../README.md)** at the repo root is the project's landing
page — read that first for what Nova is and why. Everything in this folder
is the implementation-level detail the README deliberately keeps out of the
way.

## Layout

- **[quickstart.md](quickstart.md)** — getting the binaries, running your
  first request, and the other first-hour basics a new user needs.
- **[architecture.md](architecture.md)** — how `nova-engine`, `nova-cli`,
  and `nova-app` fit together, and the boundary that keeps them consistent.
- **[reference/](reference)** — implementation-level detail for the things a
  developer needs right away: the
  [`.nova` request file format](reference/nova-file-format.md), a project's
  [directory layout and `nova.yaml`](reference/project-structure.md), the
  [CLI](reference/cli.md), and the [desktop app](reference/gui.md).
- **[project-management/](project-management)** — how contributions, issues, and
  releases actually flow. Decisions live directly as GitHub issues labeled
  `decision`, not a file in this repo — see
  [`project-management/issue-workflow.md`](project-management/issue-workflow.md)
  for the convention, or query `is:issue label:decision` on the repo directly.

Roadmap/future-feature ideas live as GitHub issues, not a doc in this repo —
see the repo's issue tracker.

More sections will be added here as the need for them comes up.
