#!/usr/bin/env node
// One-off script used to capture docs/images/*.png from the live frontend
// for the README's screenshot grid. Modeled on pipe-deck's
// scripts/screenshot-app.mjs (same maintainer, same trick): run the Vue
// frontend alone via a bare `vite` dev server (no Tauri shell, no display
// server needed) and inject a window.__TAURI_INTERNALS__ shim that answers
// every invoke() call the frontend makes with canned data, so the UI
// renders as if a real project were open — no actual Tauri backend
// involved.
import { spawn } from "node:child_process";
import { existsSync, mkdirSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "@playwright/test";

const appRoot = dirname(dirname(fileURLToPath(import.meta.url)));
const repoRoot = resolve(appRoot, "..", "..");
const imagesDir = join(repoRoot, "docs", "images");
const port = 4521;
const baseUrl = `http://localhost:${port}`;

// A realistic sample project, adapted from the shape of the engine's own
// bundled fixture at crates/nova-engine/tests/fixtures/basic-project/ (the
// project `make run`/`make validate` already use) rather than invented from
// scratch, so the request/response shapes captured here match what the real
// engine actually produces.
const projectRoot = "/home/dev/worldzero/nova";
const collectionsPath = `${projectRoot}/collections`;
const environmentsDir = `${projectRoot}/envs`;

const loginRequest = { name: "login", path: `${collectionsPath}/auth/login.nova`, method: "POST" };
const createUserRequest = { name: "create", path: `${collectionsPath}/users/create.nova`, method: "POST" };
const getUserRequest = { name: "get", path: `${collectionsPath}/users/get.nova`, method: "GET" };
const listUsersRequest = { name: "list", path: `${collectionsPath}/users/list.nova`, method: "GET" };

const collections = {
  name: "",
  path: collectionsPath,
  requests: [],
  children: [
    {
      name: "auth",
      path: `${collectionsPath}/auth`,
      requests: [loginRequest],
      children: [],
    },
    {
      name: "users",
      path: `${collectionsPath}/users`,
      requests: [createUserRequest, getUserRequest, listUsersRequest],
      children: [],
    },
  ],
};

const manifest = {
  version: 1,
  project: { name: "WorldZero API" },
  defaults: { environment: "local", timeout: null },
  collections: { path: "collections" },
  environments: { path: "envs" },
};

const environments = [
  {
    name: "local",
    variables: { base_url: "http://localhost:8080", username: "developer" },
    auth: null,
    path: `${environmentsDir}/local.yaml`,
  },
  {
    name: "staging",
    variables: { base_url: "https://staging.worldzero.example.com", username: "developer" },
    auth: null,
    path: `${environmentsDir}/staging.yaml`,
  },
];

const novaProject = {
  root: projectRoot,
  manifest,
  environments,
  environments_dir: environmentsDir,
  collections,
};

// Per-request editable drafts, keyed by the request's path — what
// `read_request` hands back to populate RequestPanel's Params/Auth/Headers/
// Body tabs.
const requestDrafts = {
  [loginRequest.path]: {
    method: "POST",
    url: "{{base_url}}/auth/login",
    query: [],
    headers: [{ name: "Content-Type", value: "application/json" }],
    body_text: JSON.stringify({ username: "{{username}}", password: "{{password}}" }, null, 2),
    auth: null,
    sync_content_type: true,
    assert_text: "status == 200\naccess_token = response.access_token",
    script_pre: null,
    script_post: null,
    has_example_response: false,
  },
  [createUserRequest.path]: {
    method: "POST",
    url: "{{base_url}}/users",
    query: [],
    headers: [
      { name: "Content-Type", value: "application/json" },
      { name: "Accept", value: "application/json" },
    ],
    body_text: JSON.stringify(
      { name: "Jane Cooper", email: "jane.cooper@example.com", role: "admin" },
      null,
      2,
    ),
    auth: { type: "bearer", token: "{{api_token}}" },
    sync_content_type: true,
    assert_text: "status == 201",
    script_pre: null,
    script_post: null,
    has_example_response: true,
  },
  [getUserRequest.path]: {
    method: "GET",
    url: "{{base_url}}/users/{{user_id}}",
    query: [],
    headers: [{ name: "Authorization", value: "Bearer {{api_token}}" }],
    body_text: "",
    auth: null,
    sync_content_type: true,
    assert_text: "user_id = response.id",
    script_pre: null,
    script_post: null,
    has_example_response: false,
  },
  [listUsersRequest.path]: {
    method: "GET",
    url: "{{base_url}}/users",
    query: [
      { name: "page", value: "1" },
      { name: "limit", value: "20" },
    ],
    headers: [{ name: "Authorization", value: "Bearer {{api_token}}" }],
    body_text: "",
    auth: null,
    sync_content_type: true,
    assert_text: "",
    script_pre: null,
    script_post: null,
    has_example_response: false,
  },
};

// send_request's response, keyed by request path.
const sendResponses = {
  [createUserRequest.path]: {
    status: 201,
    headers: [
      { name: "Content-Type", value: "application/json" },
      { name: "Location", value: "/users/1042" },
      { name: "X-RateLimit-Remaining", value: "97" },
    ],
    body: JSON.stringify(
      {
        id: 1042,
        name: "Jane Cooper",
        email: "jane.cooper@example.com",
        role: "admin",
        created_at: "2026-08-28T12:03:00Z",
      },
      null,
      2,
    ),
    elapsed_ms: 142,
  },
  [loginRequest.path]: {
    status: 200,
    headers: [{ name: "Content-Type", value: "application/json" }],
    body: JSON.stringify({ token: "eyJhbGciOi...redacted", expires_in: 3600 }, null, 2),
    elapsed_ms: 88,
  },
  [getUserRequest.path]: {
    status: 200,
    headers: [{ name: "Content-Type", value: "application/json" }],
    body: JSON.stringify(
      { id: 1042, name: "Jane Cooper", email: "jane.cooper@example.com", role: "admin" },
      null,
      2,
    ),
    elapsed_ms: 64,
  },
  [listUsersRequest.path]: {
    status: 200,
    headers: [{ name: "Content-Type", value: "application/json" }],
    body: JSON.stringify(
      {
        page: 1,
        limit: 20,
        total: 2,
        items: [
          { id: 1024, name: "Alex Rivera", email: "alex.rivera@example.com" },
          { id: 1042, name: "Jane Cooper", email: "jane.cooper@example.com" },
        ],
      },
      null,
      2,
    ),
    elapsed_ms: 51,
  },
};

// Diffing the latest send of `create.nova` against the run before it — a
// realistic "this response changed since last time" story: the new user got
// a different id, a Location header appeared, and the rate-limit counter
// ticked down.
const responseDiff = {
  status: null,
  header_changes: [
    { kind: "Added", name: "Location", value: "/users/1042" },
    { kind: "Changed", name: "X-RateLimit-Remaining", before: "98", after: "97" },
  ],
  body: {
    kind: "Json",
    changes: [
      { kind: "Changed", path: "$.id", before: 1041, after: 1042 },
      { kind: "Changed", path: "$.created_at", before: "2026-08-28T11:58:12Z", after: "2026-08-28T12:03:00Z" },
    ],
  },
  identical: false,
};

const historySummaries = [
  { id: 4, method: "GET", url: "http://localhost:8080/users?page=1&limit=20", status: 200, elapsed_ms: 51, sent_at_ms: Date.now() - 30_000 },
  { id: 3, method: "GET", url: "http://localhost:8080/users/1042", status: 200, elapsed_ms: 64, sent_at_ms: Date.now() - 90_000 },
  { id: 2, method: "POST", url: "http://localhost:8080/users", status: 201, elapsed_ms: 142, sent_at_ms: Date.now() - 150_000 },
  { id: 1, method: "POST", url: "http://localhost:8080/auth/login", status: 200, elapsed_ms: 88, sent_at_ms: Date.now() - 210_000 },
];

const historyDetails = {
  4: { request: requestDrafts[listUsersRequest.path], response: sendResponses[listUsersRequest.path] },
  3: { request: requestDrafts[getUserRequest.path], response: sendResponses[getUserRequest.path] },
  2: { request: requestDrafts[createUserRequest.path], response: sendResponses[createUserRequest.path] },
  1: { request: requestDrafts[loginRequest.path], response: sendResponses[loginRequest.path] },
};

let mockServerRunning = false;

/**
 * Handles every invoke() call the frontend makes. Keyed by command name,
 * with a couple of commands needing to look at their args (which request
 * was read/sent/diffed, which history entry was reopened) rather than
 * returning one fixed value.
 */
function handleInvoke(cmd, args) {
  switch (cmd) {
    case "plugin:dialog|open":
      return projectRoot.replace(/\/nova$/, "");
    case "open_project":
      return { found: novaProject };
    case "validate_project":
      return [];
    case "git_status":
      return null;
    case "mock_server_status":
      return { running: mockServerRunning, host: mockServerRunning ? "127.0.0.1" : null, port: mockServerRunning ? 4010 : null };
    case "start_mock_server":
      mockServerRunning = true;
      return { running: true, host: "127.0.0.1", port: 4010 };
    case "stop_mock_server":
      mockServerRunning = false;
      return { running: false, host: null, port: null };
    case "read_request":
      return requestDrafts[args.requestPath] ?? null;
    case "send_request":
      return sendResponses[args.requestPath] ?? null;
    case "diff_against_previous_run":
      return args.requestPath === createUserRequest.path ? responseDiff : null;
    case "diff_against_example_response":
      return null;
    case "get_history":
      return historySummaries;
    case "reopen_history_entry":
      return historyDetails[args.id] ?? null;
    case "parse_multipart_body":
      return [];
    case "serialize_multipart_body":
      // RequestPanel calls this unconditionally whenever `multipartFields`
      // resets to `[]` on load — including for a non-multipart request —
      // and silently swallows a failure, leaving `bodyText` untouched. None
      // of the sample requests here have a multipart body, so mirror the
      // real engine's "headers don't name a multipart boundary" rejection
      // rather than resolving and clobbering the JSON body just loaded.
      throw new Error("not a multipart body");
    case "run_tests":
      return { passed: 0, failed: 0, requests: [] };
    default:
      return null;
  }
}

function waitForServer(url, timeoutMs = 30000) {
  const deadline = Date.now() + timeoutMs;
  return new Promise((resolvePromise, reject) => {
    const attempt = () => {
      fetch(url)
        .then(() => resolvePromise())
        .catch((err) => {
          if (Date.now() > deadline) {
            reject(err);
            return;
          }
          setTimeout(attempt, 300);
        });
    };
    attempt();
  });
}

async function main() {
  if (!existsSync(imagesDir)) mkdirSync(imagesDir, { recursive: true });

  const viteBin = join(appRoot, "node_modules", ".bin", "vite");
  const vite = spawn(viteBin, ["--port", String(port), "--strictPort"], {
    cwd: appRoot,
    // "ignore" rather than "inherit" — if this script's own stdout is piped
    // (e.g. through `tail`) and vite inherited that fd directly, an
    // otherwise-quiet vite process would keep the pipe open past this
    // script's own exit, hanging the pipeline; readiness is already
    // polled via `waitForServer`, so vite's own logs aren't needed here.
    stdio: "ignore",
  });

  const cleanup = () => {
    vite.kill();
  };
  process.on("exit", cleanup);

  try {
    await waitForServer(baseUrl);

    const browser = await chromium.launch();
    const page = await browser.newPage({ viewport: { width: 1440, height: 900 } });

    await page.addInitScript(() => {
      window.__TAURI_INTERNALS__ = {
        invoke: (cmd, args) => Promise.resolve(window.__novaMockInvoke(cmd, args ?? {})),
        transformCallback: () => 0,
        unregisterCallback: () => {},
      };
    });
    // Exposed separately (rather than serialized into addInitScript) so the
    // handler can hold real functions/closures instead of just JSON data.
    await page.exposeFunction("__novaMockInvoke", handleInvoke);

    await page.goto(baseUrl, { waitUntil: "networkidle" });

    // Open the (mocked) project via the same "Open Project" button flow a
    // real user goes through: click it, and the mocked dialog + open_project
    // commands take it from there.
    await page.getByRole("button", { name: "Open Project" }).click();
    await page.waitForSelector(".collection-tree__request");

    async function shot(file) {
      await page.screenshot({ path: join(imagesDir, file) });
      console.log(`Captured ${file}`);
    }

    // 1. Request editor — open the "create user" request and land on the
    // Body tab, which shows real JSON content.
    await page.getByText("create", { exact: true }).click();
    await page.waitForSelector(".request-panel__method-url");
    await page.getByRole("tab", { name: /^Body/ }).click();
    await page.waitForTimeout(200);
    await shot("request-editor.png");

    // 2. Response view — send it, and let the Preview tab show the JSON
    // response body that comes back.
    await page.getByRole("button", { name: /^Send/ }).click();
    await page.waitForSelector(".response-summary");
    await page.waitForTimeout(200);
    await shot("response-view.png");

    // 3. Import/export dialog — a structurally different screen (a modal
    // overlay, not another view of the request/response panes) showing a
    // distinct part of the app.
    await page.getByTitle("Import / export").click();
    await page.waitForSelector(".import-export__heading");
    await page.waitForTimeout(200);
    await shot("import-export.png");
    await page.getByRole("button", { name: "Close" }).click();
    await page.waitForTimeout(100);

    // 4. History panel — the project's recent sends, master/detail — a
    // third distinct screen layout (no request/response tabs at all).
    await page.getByTitle("Request history").click();
    await page.waitForSelector(".history-panel__entry");
    await page.locator(".history-panel__entry").first().click();
    await page.waitForTimeout(200);
    await shot("history-panel.png");

    await browser.close();
  } finally {
    cleanup();
  }
}

main().catch((err) => {
  console.error(err);
  process.exitCode = 1;
});
