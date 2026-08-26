# Milestones & Releases

Three GitHub milestones currently map the engine work README's "Core Features"
section describes but `nova-engine` doesn't implement yet:

- **Core HTTP Engine** — `.http` parsing, HTTP execution, auth schemes,
  environment/secret substitution. The baseline that makes `nova run` actually
  execute a request end to end.
- **Testing & Chaining** — the assertions engine and request chaining/value
  extraction, wiring `nova test`.
- **OpenAPI & Mocking** — OpenAPI import/export and the local mock server
  (`nova mock`).

Each milestone's work is tracked as `epic`-labeled issues (native GitHub
sub-issues underneath), not as a single ticket — see the repo's issue tracker
for the current breakdown and progress.

CI/GitHub Actions (build, test, lint, issue-claiming automation, PR format
checks, etc.) are set up under `.github/workflows/` — see
[Issue Workflow](issue-workflow.md) for how claiming works now that it's
automated.
