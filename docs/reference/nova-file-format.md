# The `.nova` request file format

A `.nova` file describes one HTTP request. It's plain text, organized into
`[section]` blocks, so a diff makes it obvious which line changed — the
method/URL, a header, a body field. This is the format both the CLI and the
desktop app read and write; there is no other on-disk representation of a
request. Parsing/serialization lives in
`crates/nova-engine/src/request/parse.rs` (`parse_nova` /
`ParsedRequest::to_nova_string`).

## Sections

Only `[request]` is required. All others are optional and may appear in any
order:

| Section | Purpose |
|---|---|
| `[request]` | HTTP method and URL (required) |
| `[params]` | Query parameters |
| `[headers]` | Request headers |
| `[body]` | Request body (raw text, `multipart/form-data`, or a raw binary file) |
| `[auth]` | A structured authentication scheme |
| `[settings]` | Per-request authoring preferences (not sent on the wire) |
| `[assert]` | Test assertions and value extractions |
| `[script]` | A pre-request and/or post-response script to run |
| `[response <status>]` | A canned example response, used by `nova mock` |

### `[request]`

```text
[request]
method: POST
url: {{base_url}}/users
```

`url` is always the bare path/URL — query parameters never go here, they
belong in `[params]` — so the diff for "add a query param" only ever touches
the `[params]` section, not the request line.

### `[params]`

```text
[params]
page: 1
status: active
status: pending
```

`key: value` lines. Repeat a key for a multi-value parameter (`status` above
becomes `?status=active&status=pending`).

### `[headers]`

```text
[headers]
Authorization: Bearer {{token}}
Content-Type: application/json
```

`key: value` lines, sent verbatim (after `{{variable}}` substitution). A
literal `Authorization` header here — including the shorthand
`Basic {{username}}:{{password}}`, which gets base64-encoded automatically —
is a valid alternative to a structured `[auth]` section below.

### `[body]`

Raw request body text. For a `multipart/form-data` body, the `Content-Type`
header must carry a `boundary=` parameter, and the body itself is standard
MIME multipart wire format:

```text
[headers]
Content-Type: multipart/form-data; boundary=BOUNDARY

[body]
--BOUNDARY
Content-Disposition: form-data; name="title"

My Upload
--BOUNDARY
Content-Disposition: form-data; name="file"; filename="notes.txt"
Content-Type: text/plain

hello from a file
--BOUNDARY--
```

A multipart field can also point at a file on disk instead of embedding its
contents, using a `Content-Location` header inside the part (which itself
goes through `{{variable}}` substitution, so it can point into an
environment-specific directory):

```text
--BOUNDARY
Content-Disposition: form-data; name="file"; filename="photo.png"
Content-Type: image/png
Content-Location: {{attachments_dir}}/photo.png

--BOUNDARY--
```

The desktop app's structured multipart editor (`MultipartEditor.vue`) reads
and writes this same format; there's no separate "attachment" concept at the
file level.

A raw binary body — a file's bytes sent as the *entire* request payload,
rather than one part of a `multipart/form-data` body (e.g. `PUT /files/{id}`
with `Content-Type: application/octet-stream`) — is declared with a single
`@file:` line naming the file, resolved relative to the project root and,
like a multipart field's `Content-Location`, itself run through
`{{variable}}` substitution:

```text
[headers]
Content-Type: application/octet-stream

[body]
@file: {{attachments_dir}}/payload.bin
```

The file's contents are never inlined — they're read from disk at send
time, the same as a multipart file attachment.

A GraphQL body (`Content-Type: application/graphql+json`) uses its own tiny
nested format inside `[body]`: a raw query/mutation/subscription document,
optionally followed by a `[variables]` marker introducing its JSON
variables, and/or an `[operationName]` marker introducing its operation
name:

```text
[headers]
Content-Type: application/graphql+json

[body]
mutation CreateUser($name: String!) {
  createUser(name: $name) {
    id
  }
}

[variables]
{ "name": "John" }

[operationName]
CreateUser
```

Both markers are optional. On the wire this is assembled into the standard
`{"query", "variables", "operationName"}` JSON envelope GraphQL servers
expect; `variables` defaults to an empty object when omitted.

The desktop app can also introspect a GraphQL request's live schema, using
the request's own resolved URL/headers/auth — see
[the GUI reference](./gui.md#graphql-schema-explorer). This doesn't change
anything about the file format above; it's a read-only browsing aid.

### `[auth]`

Structured auth, as an alternative to a literal `Authorization` header.
`type:` selects the scheme; the remaining fields depend on it. Every field
goes through the same `{{variable}}` substitution as the rest of the request.

```text
[auth]
type: bearer
token: {{access_token}}
```

```text
[auth]
type: basic
username: {{username}}
password: {{password}}
```

```text
[auth]
type: api_key
name: X-API-Key
value: {{api_key}}
location: header
```

`location` is `header` (default) or `query`.

```text
[auth]
type: oauth2_client_credentials
token_url: {{token_url}}
client_id: {{client_id}}
client_secret: {{client_secret}}
scope: read write
```

`oauth2_client_credentials` exchanges the client ID/secret for a token at
`token_url` when the request is sent, applies it as a `Bearer` header, and
caches it for the rest of the run (keyed by token endpoint + client ID,
respecting the endpoint's advertised lifetime). `scope` is optional.

```text
[auth]
type: oauth2_authorization_code
auth_url: {{auth_url}}
token_url: {{token_url}}
client_id: {{client_id}}
client_secret: {{client_secret}}
scope: read write
```

`oauth2_authorization_code` is the OAuth2 grant that needs a human to log in
through a browser (RFC 6749 §4.1) rather than a client secret alone. Because
of that, a plain send can't complete it on its own the way
`oauth2_client_credentials` does — a caller has to authorize up front (in the
desktop app, the Auth tab's "Get New Access Token" button; the CLI has no
equivalent yet), which opens `auth_url` in the system browser, briefly runs a
local redirect listener to catch the authorization code, and exchanges it for
an access token. That token is cached the same way a client-credentials
token is (by token endpoint, client ID, and scope) and reused by any request
declaring a matching `oauth2_authorization_code` scheme until it expires. A
request sent before anything has authorized it fails with a clear error
rather than hanging or silently going out unauthenticated. `scope` is
optional.

```text
[auth]
type: digest
username: {{username}}
password: {{password}}
```

`digest` is HTTP Digest authentication (RFC 7616), MD5 only. A request
declaring it goes out once with no `Authorization` header; only if the
server answers `401` with a `WWW-Authenticate: Digest` challenge is a
response computed from the challenge and the request retried once with it.
A digest challenge advertising a different algorithm (`MD5-sess`, `SHA-256`,
`SHA-512-256`) is reported as an error rather than silently producing a
response the server will reject.

An environment's own `auth:` block (see below) can supply a default scheme
that a request doesn't declare one itself; a request's own `[auth]` always
wins, and an environment default never overwrites a header the request set
by hand.

### `[settings]`

Authoring preferences — nothing here goes on the wire. Currently just:

```text
[settings]
sync_content_type: false
```

`sync_content_type` (default `true`) controls whether picking a body type in
the desktop app also rewrites the `Content-Type` header. Turn it off for a
request that deliberately pairs a custom content type with a
differently-shaped body.

### `[assert]`

Test assertions, and extractions that later requests in the same run can
reference as `{{variable}}`s (see
`crates/nova-engine/src/execution/assertion.rs`).

```text
[assert]
status == 201
response.user.id exists
response.user.email == input.email
response.time < 500ms
access_token = response.access_token
```

- `<term> <op> <term>` — comparison (`==`, `!=`, `<`, `>`, `<=`, `>=`)
- `<term> exists` — presence check
- `<name> = <term>` — extraction: stores the resolved value as `{{name}}`
  for later requests in the same run (e.g. chaining a login's
  `access_token` into a subsequent request's `Authorization` header)

Valid terms include `status`, `response.time` (with a unit, e.g. `500ms`),
`response.<json.path>`, and `input.<field>` (a value from the request's own
body).

**Request chaining.** An extraction's value is only held for the rest of
that run (one `nova run`/`nova test` invocation, or one Send in the desktop
app's `Session`) — it isn't written back to disk. This lets a whole
workflow reference values from earlier steps without manual copy-pasting:

```text
Login  →  extract access_token  →  Create User  →  extract user_id  →  Get User
```

```text
[request]
method: POST
url: {{base_url}}/auth/login

[body]
{ "username": "{{username}}", "password": "{{password}}" }

[assert]
access_token = response.access_token
```

```text
[request]
method: POST
url: {{base_url}}/users

[headers]
Authorization: Bearer {{access_token}}
```

### `[script]`

Names a pre-request and/or post-response script to run around this
request's execution (see `crates/nova-engine/src/execution/script.rs`):

```text
[script]
pre: sign-request
post: log-response
```

- `pre:` runs after `{{variable}}` resolution but before the request is
  sent. It can add or override headers/query params, or replace the
  outgoing body.
- `post:` runs after the response comes back. It can extract variables
  that later requests in the same run can reference as `{{variable}}`s,
  the same way an `[assert]` extraction does.

A bare name (no path separator, e.g. `sign-request`) resolves against the
project's `nova/scripts/` directory; an explicit path (e.g.
`../shared/sign.py`) is resolved relative to the project root instead. The
file extension picks the interpreter the engine shells out to — `.js`,
`.mjs`, and `.ts` run under `node`, `.py` runs under `python3`, and the
mapping is easy to extend. A script's extension having no configured
interpreter (or the interpreter not being installed) is an error, not a
silent no-op.

The contract is the same regardless of language: the engine writes a JSON
payload to the script's stdin and reads a JSON response from its stdout.
A pre-request script receives the outgoing request's method, URL, query
params, headers, and body, and may return:

```json
{ "headers": { "X-Signature": "..." }, "params": { "...": "..." }, "body": "..." }
```

A post-response script receives the response (status, headers, body,
timing) and returns the variables it extracted, by name:

```json
{ "access_token": "..." }
```

There is no sandboxing beyond the OS process: running a script from a
cloned project is the same trust decision as running that project's
Makefile or pre-commit hooks.

A collection directory can also carry its own `[script]`-style association
that applies to every request nested under it, not just this one — see
[Project structure](./project-structure.md#collection-scoped-scripts). A
collection-level `pre:` runs before this request's own, and a
collection-level `post:` runs after it, nesting outermost-scope-first.

### `[response <status>]`

An example response, used only by `nova mock` — never sent or received by
`nova run`/`nova test`. The status code comes from the section marker itself
and defaults to `200` when omitted:

```text
[response 201]
Content-Type: application/json

{ "id": "usr_1234", "name": "John" }
```

A request with no `[response]` section still gets a mock route; it answers
`501` explaining that no example response is defined, rather than being
silently excluded from the mock server.

Hand-writing this section works, but it's also written for you: sending a
request and choosing "Save as Example" in the desktop app's response pane
(or running `nova run --save-example`) captures the actual response —
status, headers, body — into this section, replacing whatever was there
before and leaving the rest of the file alone.

## WebSocket requests

A `.nova` file can declare a WebSocket connection instead of an HTTP
request, by setting `protocol: websocket` under `[request]`. Only `url`,
`[headers]`, and `[messages]` apply — there's no method, params, body,
auth, assertions, or example response section for a WebSocket request:

```text
[request]
protocol: websocket
url: {{ws_base_url}}/echo

[headers]
Authorization: Bearer {{auth_token}}

[messages]
hello
world
@file: fixtures/payload.bin
```

`[messages]` lists messages to send, one per line, in order, once the
connection opens. Most lines are sent as a text frame verbatim; a line
starting with `@file: <path>` instead sends that file's raw bytes as a
single binary frame — `path` resolved relative to the project root (and
run through `{{variable}}` substitution first), the same convention and
escape-checked resolution an HTTP `[body]`'s `@file:` binary body already
uses. `nova ws` (see the [CLI reference](./cli.md)) sends every message in
order, then collects whatever comes back until the connection closes or a
read timeout elapses; a received binary frame prints as its byte length
rather than attempting to render arbitrary bytes as text. Ping/pong frames
are still out of scope — only text and binary messages are sent or
collected. The desktop app's WebSocket panel treats this same list as a
saved-messages picker for its interactive composer (see
[the GUI reference](./gui.md)) rather than something it sends in one
batch — saving or updating a message from the composer just
appends-or-replaces an entry here, so the on-disk shape stays exactly this
plain one-message-per-line list either way. A received binary frame in the
live transcript can be saved to disk from there.

## Server-Sent Events requests

A `.nova` file can declare a Server-Sent Events (SSE) connection instead of
an HTTP request, by setting `protocol: sse` under `[request]`. SSE is
always a GET per spec, so there's no method — only `url` and `[headers]`
apply; there's no params, body, auth, assertions, or example response
section for an SSE request:

```text
[request]
protocol: sse
url: {{base_url}}/events

[headers]
Authorization: Bearer {{auth_token}}
```

`nova sse` (see the [CLI reference](./cli.md)) connects and reads the
response body incrementally, parsing the SSE event framing (`event:`/
`data:`/`id:`/`retry:` lines terminated by a blank line) as events arrive
rather than buffering the whole response, printing each event as it comes
in until the connection closes or a read timeout elapses.

## Variable substitution

Any `{{name}}` in any section — URL, params, headers, body, auth fields,
even a multipart part's `Content-Location` or a binary body's `@file:`
path — is resolved against the active
environment's `variables:` (see
[`nova.yaml` and environments](./project-structure.md)) plus any values
extracted by earlier requests' `[assert]` sections in the same run.

### Built-in dynamic placeholders

A `{{$name}}` placeholder (the leading `$` is what marks it as dynamic) is
computed fresh on every send instead of being looked up anywhere — no
environment entry needed:

| Placeholder        | Value                                                |
| ------------------- | ----------------------------------------------------- |
| `{{$uuid}}`         | A random UUID (v4)                                    |
| `{{$timestamp}}`    | The current Unix timestamp, in seconds                |
| `{{$randomInt}}`    | A random integer between 0 and 1000                   |
| `{{$randomEmail}}`  | A random `local@example.com` throwaway address        |

## Full example

```text
[request]
method: POST
url: {{base_url}}/users

[params]
notify: true

[headers]
Authorization: Bearer {{access_token}}
Content-Type: application/json

[body]
{
  "name": "John",
  "email": "john@example.com"
}

[assert]
status == 201
response.id exists
user_id = response.id

[response 201]
Content-Type: application/json

{ "id": "usr_1234", "name": "John", "email": "john@example.com" }
```
