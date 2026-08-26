# Nova — Open Source API Development Client

## Project Overview

**Nova** is an open-source, local-first API development client designed as an alternative to tools such as Postman.

The core philosophy is simple:

> **API requests are project artifacts. They should live with the code.**

Modern API clients increasingly organize requests around proprietary cloud workspaces, collections, accounts, and synchronization systems. This can conflict with normal developer workflows where code, configuration, tests, documentation, and review already happen through Git.

Nova treats API definitions as normal, human-readable files that can be committed directly to a repository.

```text
my-project/
├── src/
├── nova/
│   ├── auth/
│   │   ├── login.http
│   │   └── refresh.http
│   ├── users/
│   │   ├── create.http
│   │   └── get.http
│   └── environments/
│       ├── local.yaml
│       └── staging.yaml
└── nova.yaml
```

Clone the repository and you have the API workspace.

No separate account. No workspace invitation. No exporting/importing collections. No proprietary synchronization layer.

**Git is the collaboration system.**

---

## Goals

Nova should provide a fast, polished API development experience while remaining local-first and developer-controlled.

The primary goals are:

* Store requests in human-readable, version-controlled files.
* Make API requests easily shareable through Git.
* Provide both a graphical desktop application and CLI.
* Keep the CLI and GUI backed by the exact same project format and execution engine.
* Require no Nova account or hosted service.
* Work naturally with existing repositories and development workflows.
* Make request files readable and editable without Nova installed.
* Support automation and CI/CD as first-class workflows.
* Remain fully usable offline.

Nova should feel less like a SaaS workspace and more like another development tool living alongside Git, an editor, and a terminal.

---

# Core Architecture

Nova should be built around a reusable **Nova Engine** rather than placing request execution logic directly inside the desktop application.

```text
                 Nova Project
                      │
                 nova.yaml
                      │
                Nova Engine
               /           \
              /             \
        Nova CLI         Nova Desktop
```

The engine owns:

* project parsing
* request parsing
* environments
* variable resolution
* HTTP execution
* authentication
* scripting
* assertions
* request chaining
* response handling

This allows the same request to execute identically from the GUI:

```text
▶ Send
```

or terminal:

```bash
nova run nova/users/create.http
```

or CI:

```bash
nova test nova/
```

The desktop application is therefore primarily a visual interface over the same engine available to automation.

---

# Request Format

Requests should favor a simple text-based format.

For example:

```http
POST {{base_url}}/users
Authorization: Bearer {{token}}
Content-Type: application/json

{
  "name": "John",
  "email": "john@example.com"
}
```

Environment configuration could remain equally straightforward:

```yaml
name: local

variables:
  base_url: http://localhost:8080
```

Secrets should be separable from committed environment configuration so credentials do not need to enter Git.

Nova should prefer established formats where practical rather than creating proprietary formats simply for the sake of ownership.

---

# Core Features

### API Protocols

Initial focus:

* HTTP / REST
* JSON, XML, form and multipart bodies
* common authentication schemes
* environment variables
* cookies
* file uploads

Future protocol support:

* WebSockets
* GraphQL
* gRPC
* Server-Sent Events

### Environments

Projects can define environments such as:

```text
local
development
staging
production
```

Switching environments changes variables without modifying requests.

Secrets can be supplied locally, through environment variables, or eventually through external secret providers.

### Request Chaining

Requests should be able to consume values produced by previous requests.

Example:

```text
Login
  ↓
extract access_token
  ↓
Create User
  ↓
extract user_id
  ↓
Get User
```

This makes realistic API workflows reproducible rather than requiring developers to manually copy values between requests.

### Testing & Assertions

Requests can contain assertions such as:

```text
status == 200
response.user.id exists
response.user.email == input.email
response.time < 500ms
```

placed after a `###` line following the request body:

```http
POST {{base_url}}/users
Content-Type: application/json

{ "email": "john@example.com" }

###

status == 201
response.email == input.email
```

The same assertions run through both Desktop and CLI.

This turns Nova collections into lightweight integration/API tests.

### OpenAPI

Nova should eventually provide strong OpenAPI integration:

```text
OpenAPI → Nova Project
Nova Project → OpenAPI
```

An existing API specification could immediately generate browsable and executable requests.

### Mocking

Nova could expose project definitions as a local mock server:

```bash
nova mock
```

allowing frontend development before a backend implementation is complete.

---

# Git-Native Collaboration

This is Nova's defining feature.

A developer changes an API:

```text
POST /users

→

POST /v2/users
```

The corresponding Nova request changes in the same pull request.

Reviewers can see:

```diff
- POST {{base_url}}/users
+ POST {{base_url}}/v2/users
```

API changes therefore participate naturally in:

* branches
* pull requests
* code review
* merge conflicts
* tags
* releases
* CI/CD

There is no separate Nova collaboration model to learn.

Nova complements the workflow developers already use.

---

# Desktop Experience

The desktop application should provide the convenience expected from a modern API client:

```text
┌─────────────────────────────────────────────────────┐
│ NOVA                       LOCAL ▼                  │
├────────────────┬────────────────────────────────────┤
│ Auth           │ POST  {{base_url}}/users      SEND │
│   Login        ├────────────────────────────────────┤
│   Refresh      │ Params │ Headers │ Body │ Auth     │
│                │                                    │
│ Users          │ {                                  │
│   Create       │   "name": "John"                  │
│   Get          │ }                                  │
│   Delete       │                                    │
│                ├────────────────────────────────────┤
│ Orders         │ RESPONSE                     200   │
│   Create       │ 84ms                               │
│   List         │                                    │
│                │ { "id": "usr_1234" }              │
└────────────────┴────────────────────────────────────┘
```

The GUI should make common workflows easy without hiding or replacing the underlying files.

Advanced users should always be able to open those files directly in their editor.

---

# CLI

The CLI makes Nova useful beyond its desktop application.

```bash
nova init

nova run nova/auth/login.http

nova run nova/users/

nova test

nova test --environment staging

nova mock

nova validate
```

This makes Nova usable locally, over SSH, inside containers, and within CI/CD pipelines.

---

# Longer-Term Opportunities

Once the core HTTP workflow is excellent, Nova could expand into:

* API documentation generation
* contract testing
* schema validation
* performance/load testing
* request profiling
* API diffing
* automated OpenAPI validation
* plugin/extensions system
* shared organization standards
* secret-manager integrations
* GitHub/GitLab CI integrations
* headless Docker execution

These should remain secondary to Nova's central promise.

---

# Project Philosophy

Nova should avoid becoming a Postman clone simply because Postman has a particular feature.

Every feature should answer:

> **Does this improve developing, testing, understanding, or sharing an API?**

The project should remain:

**Open source. Local first. Git native. Human readable. Automation friendly. Developer owned.**

The ultimate workflow should be remarkably simple:

```bash
git clone project
cd project

nova open
```

And everything another developer needs to explore and test that project's API is already there.

**No workspace invitation required.**
