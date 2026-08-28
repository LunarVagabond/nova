# Documentation

The single source of truth for Nova's vision and architecture is
**[README.md](../README.md)** at the repo root — read that first. Everything in
this folder either extracts one piece of it in more detail, or will hold real
implementation-level content once there's more to document.

## Layout

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

More sections will be added here as the need for them comes up — see the
README's "Longer-Term Opportunities" for what's next once the core HTTP
workflow is solid.
