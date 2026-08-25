# Issue Workflow

**Status:** decision issues and work-item ticket quality are documented below.
Still missing: day-to-day triage/labeling mechanics and how claiming works once
it's automated (there's no CI/Actions set up for Nova yet — see
[Milestones & Releases](milestones-and-releases.md) — so claiming is currently a
manual courtesy convention; see `.github/CONTRIBUTING.md`).

## Decision issues

There is no decision-log file anywhere in this repo, on purpose — decisions live
directly as GitHub issues labeled `decision`, full stop. The whole log is just
this query: **`is:issue label:decision`**.

**The convention:**

- **Each decision is its own issue** — never just the `decision` label slapped
  onto whatever issue happened to contain the discussion.
- Use the `Decision` issue template (`.github/ISSUE_TEMPLATE/decision.yml`), which
  has fields for Context, Decision, Why, Alternatives considered, Consequences,
  and Related.
- **Open + `decision` label = still being discussed. Closed + `decision` label =
  decided.** Close the issue the moment the decision is made — that state
  transition *is* the signal.
- **Never rewrite a past decision issue.** If a decision changes later, open a
  new issue with `Supersedes #<old issue>` in it, and note
  `Superseded by #<new issue>` back on the old one. History stays visible even
  after it's no longer current.

Filing a decision issue is deliberately a little more friction than firing off a
comment — that's intentional, not an oversight. It's a natural forcing function
to actually think about whether something needs to be a tracked decision at all,
versus just... doing it.

## Work-item ticket quality

No dedicated template for implementation work items — use `Feature request` (or
`Bug report`/`Decision` when those fit better), same three templates as
everything else. The bar is about the body content, not the form:

An outside contributor with zero other context should be able to read a ticket
and have little to no question about what's actually being asked for. A title
plus "see the README" is not a ticket — it's a label. Concretely, a well-written
work-item ticket includes:

- **What needs to be built** — concrete enough to start without a clarifying
  question. Not "parse `.http` files," but what the resulting struct/fields look
  like, which HTTP methods and body types are in scope, what's explicitly not
  handled yet.
- **Context / why** — link the relevant `README.md` section and explain the
  "why" if it isn't obvious from the section alone.
- **Acceptance criteria** — specific, checkable conditions. Not "it works" —
  what "it works" actually means here, as a checklist.
- **Out of scope**, when it isn't obvious — so it doesn't quietly absorb an
  adjacent ticket's work.
- **Related** — the epic it belongs to, other issues, relevant README section.

This applies whether the issue is filed by a person or an AI agent working in
this repo — see the AI-agent note in `.github/CONTRIBUTING.md`.

**Titles carry the template's prefix.** Each issue template already defines one
(`.github/ISSUE_TEMPLATE/*.yml`'s `title:` field) — keep it when filing, don't
strip it out: `[Feature] `, `[Bug] `, `[Decision] `. Every new issue's title
should start with the prefix matching whichever template it came from.

**Don't prefix the title with a crate name.** `crate: <name>` labels exist for
that — a title like `[Feature] nova-engine: parse .http bodies` duplicates what
the label already says and just adds noise. Write
`[Feature] Parse .http request bodies` and set the `crate: nova-engine` label
instead. Same goes for PR titles (`[#<issue>] - <short description>` in
`.github/CONTRIBUTING.md`) — no crate prefix there either, the linked issue's
label already carries that.
