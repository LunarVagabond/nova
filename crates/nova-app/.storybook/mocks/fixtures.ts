// Shared sample data for Tauri IPC mocks (see ./tauri-ipc.ts) and for
// story `args` generally. Add to this file rather than inlining ad hoc
// literals in individual `.stories.ts` files, so components that share a
// data shape (e.g. `HistorySummary` used by both `HistoryPanel` and a
// future history-aware component) render against the same sample values.

import type {
  CookieView,
  HistoryDetail,
  HistorySummary,
  MockCallLogEntry,
  RequestDraft,
  RequestResponse,
  ResolvedVariables,
} from "../../src/types/nova";

export const sampleResolvedVariables: ResolvedVariables = {
  variables: {
    base_url: "https://api.example.com",
    api_version: "v2",
  },
  secrets: ["api_key"],
};

export const sampleRequestDraft: RequestDraft = {
  method: "GET",
  url: "{{base_url}}/users/{{user_id}}",
  query: [{ name: "expand", value: "profile" }],
  headers: [{ name: "Accept", value: "application/json" }],
  body_text: "",
  auth: { type: "bearer", token: "{{api_key}}" },
  sync_content_type: true,
  assert_text: "",
  script_pre: null,
  script_post: null,
  has_example_response: false,
  example_responses: [],
};

export const sampleRequestResponse: RequestResponse = {
  status: 200,
  headers: [{ name: "Content-Type", value: "application/json" }],
  body: '{"id":"u_123","name":"Ada Lovelace"}',
  elapsed_ms: 142,
  timing: { time_to_first_byte_ms: 118, content_download_ms: 24 },
};

export const sampleHistorySummaries: HistorySummary[] = [
  {
    id: 3,
    method: "GET",
    url: "https://api.example.com/users/u_123",
    status: 200,
    elapsed_ms: 142,
    sent_at_ms: Date.now() - 1000 * 60 * 5,
  },
  {
    id: 2,
    method: "POST",
    url: "https://api.example.com/users",
    status: 201,
    elapsed_ms: 310,
    sent_at_ms: Date.now() - 1000 * 60 * 40,
  },
  {
    id: 1,
    method: "GET",
    url: "https://api.example.com/users/u_999",
    status: 404,
    elapsed_ms: 88,
    sent_at_ms: Date.now() - 1000 * 60 * 60 * 3,
  },
];

export const sampleHistoryDetail: HistoryDetail = {
  request: sampleRequestDraft,
  response: sampleRequestResponse,
};

export const sampleCookies: CookieView[] = [
  {
    host: "api.example.com",
    name: "session_id",
    value: "s%3AabcDEF123",
    path: "/",
    secure: true,
    domain: null,
    expires_at_ms: Date.now() + 1000 * 60 * 60 * 24,
  },
  {
    host: "api.example.com",
    name: "csrf_token",
    value: "tok_9f8e7d",
    path: "/",
    secure: false,
    domain: ".example.com",
    expires_at_ms: null,
  },
];

export const sampleMockCallLog: MockCallLogEntry[] = [
  {
    id: 2,
    received_at_ms: Date.now() - 1000 * 30,
    method: "GET",
    path: "/users/u_123",
    matched_route: "GET /users/:id",
    status: 200,
  },
  {
    id: 1,
    received_at_ms: Date.now() - 1000 * 90,
    method: "POST",
    path: "/webhook",
    matched_route: null,
    status: 404,
  },
];
