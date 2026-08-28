# Contributing

## Purpose

Contribution standards for Nova, with emphasis on clarity, and staying true to the
project's Git-native, local-first philosophy.

## Feature Proposal Gate

Each feature proposal or implementation should answer:

- Does this improve developing, testing, understanding, or sharing an API — per
  Nova's [Philosophy](../README.md#philosophy)?
- Does it keep human-readable project files as the source of truth, with the engine
  (not the CLI or GUI) owning parsing/discovery/execution — per
  [Architecture](../docs/architecture.md)?

If no, refine or drop the proposal.

## Questions before you file

For a quick question or a sanity check before opening an issue, drop into
[Discord](https://discord.gg/cHtuCFkRRm) (server name: "Dev Syndicate"). For
anything worth keeping searchable — a design question, a proposal — use GitHub
Discussions instead.

If you're using Nova on a real project and hit something it doesn't support yet,
that's exactly the kind of report this project wants — open an issue or a
Discussion rather than working around it quietly or forking.

## If A Convention Gets In The Way

The branching model, commit format, and process rules below are a starting point,
not a settled standard. Follow them as written. But if one is genuinely getting in
the way of a contribution, doesn't fit a situation, or just seems off, raise it
first — a GitHub Discussion or a note in [Discord](https://discord.gg/cHtuCFkRRm) —
before working around it. Same goes for friction in the tools, the codebase, or the
workflow generally: surfacing it is always welcome. The goal is to talk it through
and adjust the rule if it's wrong, not to greenlight quietly deviating from it.

## Contribution Principles

Straight from Nova's [Why Nova](../README.md#why-nova) and
[Philosophy](../README.md#philosophy):

- API requests are project artifacts — human-readable files that live with the
  code, not entries in a proprietary workspace database.
- Git is the collaboration system. Nova does not build its own sync/collaboration
  layer on top of it.
- The engine owns the product; the CLI and GUI are thin, interchangeable interfaces
  over it — never reimplement parsing/discovery/execution in either.
- No Nova account or hosted service, ever, for the core workflow.
- Fully usable offline.

## Branching

- `main` — integration branch
- `<issue#>-short-description` — topic branches off `main`, named after the GitHub
  issue number (e.g. `12-http-request-parser`); no `feature/`, `bug/`, or similar
  prefix, the issue number is the lookup
- `noissue-short-description` — maintainer-only, mirroring the `[noissue]`
  commit/PR restriction below. If you see a branch like this, it's a maintainer
  quick fix, not a pattern open to other contributors
- `hotfix-short-description` — maintainer-only, mirroring the `[hotfix]`
  commit/PR restriction below. If you see a branch like this, it's a maintainer
  hotfix, not a pattern open to other contributors

## Work Tracking

Open work lives in GitHub Issues. Product direction lives in
[`README.md`](../README.md); acceptance criteria for specific initiatives live on
their tracking issue/epic, not in a docs file. Completed history is in git; do not
maintain a separate backlog file in the repo.

### Claiming An Issue

Before starting work, comment on the issue to say you're picking it up, and wait
for a maintainer to assign it to you — that assignment is what actually reserves
it, so someone else doesn't start the same ticket in parallel. If an issue is
already assigned, treat it as taken; comment to ask if it looks stalled instead of
opening a competing PR. Epics don't work this way — find the specific sub-issue you
want and claim that instead.

*This is enforced automatically, not just a courtesy convention — see
[Issue Workflow](../docs/project-management/issue-workflow.md) for how claiming and
the check that verifies it actually work.*

## Commits And Pull Requests

Open an issue first when the work is non-trivial. The issue carries context
(feature, bug, scope) — commits and PRs reference it by number.

### Commit Messages

Merges into `main` are **squash-only** — your branch's individual commits never
appear in `main`'s history, only the squashed PR title does (see
[Pull Request Titles](#pull-request-titles) below, which *is* strict). Because of
that, commit messages on your branch are a suggested convention, not a
requirement: write them however helps you work, `wip`/`fixup`/whatever included.

If you'd like to follow the convention anyway (it makes review easier), it's the
same pattern as PR titles:

```
[#<issue>] - <short description>
```

**`[noissue]`, `[hotfix]`, and `[security]` are restricted.** All three exist only
for the maintainer, a small, explicitly-named set of trusted core developers, and
(for `[security]`/`[noissue]`) Dependabot. If you are not on that short list, use
your issue number when you do tag commits. The tags mean different things:

- `[noissue]` — trivial, no ticket is warranted at all (typo, comment, one-line
  fix). Also what Dependabot's routine scheduled dependency bumps carry.
- `[hotfix]` — must be fixed now and there's a clear path to the fix, but there
  wasn't time to write up a ticket first. Reaching for this signals "this was a
  real bug/issue," not "there was nothing to file."
- `[security]` — a fix for a known vulnerability, most commonly Dependabot's
  security-triggered updates, occasionally a manual CVE/advisory fix.

Examples:

- `[#12] - Add .http request parser and structured request model`
- `[#12] - Wire request parsing into nova-cli's inspect command`
- `[noissue] - Fix typo in Contributing commit examples` (maintainer/core-only)
- `[hotfix] - Guard against panic on empty collections directory` (maintainer/core-only)

### Pull Request Titles

**This one is a hard requirement, unlike commit messages above.** Merges are
squash-only, so the PR title becomes the actual commit message on `main` — it's
the one place this format has to be right.

```
[#123] - Add .http request parser and structured request model
[noissue] - Fix typo in README quick start
[hotfix] - Guard against panic on empty collections directory
[security] - Bump a dependency to patch a known CVE
```

`[noissue]`, `[hotfix]`, and `[security]` follow the same restriction as commit
messages above — maintainer, named core developers, and Dependabot only. Everyone
else opens an issue first and references it in the title. The PR body can go
deeper on approach and testing.

### AI-Assisted Contributions

AI coding assistants are welcome as a tool — this is not the same as "vibe coding"
(accepting AI output wholesale without understanding or reviewing it). If an
assistant materially helped with a commit, tag it with a trailer so it's easy to
trace later, without cluttering the subject line:

```
git commit -m "[#42] - add environment variable resolution" --trailer "Co-Authored-By: Claude <noreply@anthropic.com>"
git commit -m "[#7] - fix collection discovery on symlinked dirs" --trailer "Co-Authored-By: GitHub Copilot <noreply@github.com>"
```

This is optional and about being open, not a requirement — reviewers still hold
the contributor responsible for understanding and standing behind the change
either way.

#### If You Are An AI Agent Reading This

Follow the conventions in this file the same as any contributor would:
`[#<issue>] - <short description>` commit and PR titles, one logical change per
commit, docs updated alongside behavior changes. In addition:

- **Never use `[noissue]` or `[hotfix]`, and never use a `noissue-*` or
  `hotfix-*` branch name.** All are restricted to the maintainer and a small
  named set of core developers — every commit, PR, and branch you make needs a
  real issue number. If no issue exists yet for the work, that's a sign to open
  one first, not to reach for `[noissue]`/`[hotfix]`.
- Apply the `Co-Authored-By: <Tool> <email>` trailer above to every commit and PR
  you create or materially author.
- Don't add any other AI-attribution mention beyond that single trailer line
  unless explicitly asked to.
- If you're unsure whether the trailer applies in a given situation, ask rather
  than guessing.
- **Never reach for a lint/format suppression just to make a check pass.** A
  suppression without a genuine, specific justification comment is not an
  acceptable way to close out a failure; fix the underlying code instead, or ask
  if the rule itself seems wrong.
- **When filing a work-item ticket, meet the bar in
  [Issue Workflow](../docs/project-management/issue-workflow.md#work-item-ticket-quality):**
  what needs to be built, why, and real checkable acceptance criteria. A title
  plus a one-line pointer to the README is not enough — someone with no other
  context should be able to pick it up.

## Documentation-First Workflow

For major work:

1. Update the relevant file in `docs/` first (once it covers the area in
   question — much of `docs/` is still placeholder; see its `README.md`).
2. Align implementation tasks with accepted docs.
3. Update docs and behavior together on changes.

## Development Interface

The `Makefile` at the repo root is the canonical entry point for local dev
commands — run `make help` for the full list. The common ones:

```
make build        # build nova-engine + nova-cli (debug)
make test         # run nova-engine's test suite
make lint         # cargo clippy --workspace --all-targets -- -D warnings
make fmt-check    # cargo fmt --all -- --check
make run          # run the CLI against the bundled example fixture project
make dev          # run the Tauri desktop app in dev mode
```

## Where To Contribute

- New here? Start at [`README.md`](../README.md) for the project vision and
  architecture.
- Decisions: [issues labeled `decision`](https://github.com/LunarVagabond/nova/issues?q=is%3Aissue+label%3Adecision)
  (see [Issue Workflow](../docs/project-management/issue-workflow.md) for the
  convention — there is no decisions file in this repo, on purpose)
- Contributor process: this file, and the rest of
  [`docs/project-management/`](../docs/project-management)

`docs/` is a normal, PR-able part of this repo — edit it the same way as any
other change.

## Code Of Conduct

Participation in this project is governed by our
[Code of Conduct](CODE_OF_CONDUCT.md).

## OSS Onboarding Expectations

Contributions should include:

- Problem statement in plain language.
- Scope (in/out).
- Risks and rollback considerations.
- How this helps a developer avoid building or wiring up their own bespoke API
  client (or a hosted workspace they don't control) for their project.
