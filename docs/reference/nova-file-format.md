# The `.nova` request file format

A `.nova` file describes one HTTP request. It's plain text, organized into
`[section]` blocks, so a diff makes it obvious which line changed — the
method/URL, a header, a body field. This is the format both the CLI and the
desktop app read and write; there is no other on-disk representation of a
request. Parsing/serialization lives in
`crates/nova-engine/src/request.rs` (`parse_nova` /
`ParsedRequest::to_nova_string`).

## Sections

Only `[request]` is required. All others are optional and may appear in any
order:

| Section | Purpose |
|---|---|
| `[request]` | HTTP method and URL (required) |
| `[params]` | Query parameters |
| `[headers]` | Request headers |
| `[body]` | Request body (raw text, or `multipart/form-data`) |
| `[auth]` | A structured authentication scheme |
| `[settings]` | Per-request authoring preferences (not sent on the wire) |
| `[assert]` | Test assertions and value extractions |
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
reference as `{{variable}}`s (see `crates/nova-engine/src/assertion.rs`).

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
```

`[messages]` lists text messages to send, one per line, in order, once the
connection opens. `nova ws` (see the [CLI reference](./cli.md)) sends them,
then collects whatever text messages come back until the connection closes
or a read timeout elapses. Binary/ping/pong frames are out of scope for now
— only text messages are sent or collected.

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
even a multipart part's `Content-Location` — is resolved against the active
environment's `variables:` (see
[`nova.yaml` and environments](./project-structure.md)) plus any values
extracted by earlier requests' `[assert]` sections in the same run.

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
