// Pure helpers for interpreting/driving a request's Body tab — kept out of
// RequestPanel.vue so this logic can be unit tested and read on its own,
// independent of Vue reactivity.
import type { RequestHeader } from "../types/nova";
import type { EditorLanguage } from "../components/CodeEditor.vue";

// The top-level body shape, selected via a radio group rather than a single
// dropdown — "raw" covers what used to be separate json/xml/text options,
// now distinguished by `RawLanguage` instead. GraphQL and multipart-form
// each get their own dedicated editor; "none" and "form" need no further
// distinction.
export type BodyType = "none" | "form" | "multipart" | "raw" | "graphql";

export const BODY_TYPE_OPTIONS: BodyType[] = ["none", "form", "multipart", "raw", "graphql"];

export const BODY_TYPE_LABELS: Record<BodyType, string> = {
  none: "None",
  form: "x-www-form-urlencoded",
  multipart: "Form Data",
  raw: "Raw",
  graphql: "GraphQL",
};

export const BODY_TYPE_CONTENT_TYPES: Record<Exclude<BodyType, "none" | "raw">, string> = {
  form: "application/x-www-form-urlencoded",
  multipart: "multipart/form-data",
  graphql: "application/graphql+json",
};

// The language sub-choice shown only when "Raw" is selected — reuses
// `EditorLanguage` directly rather than a separate parallel type, since the
// two sets are identical.
export type RawLanguage = EditorLanguage;

export const RAW_LANGUAGE_OPTIONS: RawLanguage[] = ["text", "javascript", "json", "html", "xml"];

export const RAW_LANGUAGE_LABELS: Record<RawLanguage, string> = {
  text: "Text",
  javascript: "JavaScript",
  json: "JSON",
  html: "HTML",
  xml: "XML",
};

export const RAW_LANGUAGE_CONTENT_TYPES: Record<RawLanguage, string> = {
  text: "text/plain",
  javascript: "application/javascript",
  json: "application/json",
  html: "text/html",
  xml: "application/xml",
};

/** Infers a body type (and, for "raw", its language) from the Content-Type header and whether there's any body text at all. */
export function detectBodyType(
  currentHeaders: RequestHeader[],
  text: string,
): { type: BodyType; rawLanguage: RawLanguage } {
  const NONE = { type: "none" as const, rawLanguage: "text" as const };
  if (text.trim() === "") return NONE;

  const contentType = currentHeaders.find((h) => h.name.toLowerCase() === "content-type")?.value ?? "";
  const essence = contentType.split(";")[0]?.trim().toLowerCase() ?? "";

  if (essence === "application/x-www-form-urlencoded") return { type: "form", rawLanguage: "text" };
  if (essence === "multipart/form-data") return { type: "multipart", rawLanguage: "text" };
  if (essence === "application/graphql+json") return { type: "graphql", rawLanguage: "text" };
  if (essence === "application/json" || essence.endsWith("+json")) return { type: "raw", rawLanguage: "json" };
  if (essence === "application/xml" || essence === "text/xml" || essence.endsWith("+xml")) {
    return { type: "raw", rawLanguage: "xml" };
  }
  if (essence === "application/javascript" || essence === "text/javascript") {
    return { type: "raw", rawLanguage: "javascript" };
  }
  if (essence === "text/html") return { type: "raw", rawLanguage: "html" };
  return { type: "raw", rawLanguage: "text" };
}

export function randomBoundary(): string {
  return `----NovaBoundary${Math.random().toString(16).slice(2)}`;
}

// Shared between the request body editor and the response viewer: a
// `+json`/`+xml` structured syntax suffix (e.g. `application/vnd.api+json`)
// is treated the same as the bare media type.
export function languageForContentType(contentType: string): EditorLanguage {
  const essence = contentType.split(";")[0]?.trim().toLowerCase() ?? "";
  if (essence === "application/json" || essence.endsWith("+json")) return "json";
  if (essence === "application/xml" || essence === "text/xml" || essence.endsWith("+xml")) return "xml";
  if (essence === "application/javascript" || essence === "text/javascript") return "javascript";
  if (essence === "text/html") return "html";
  return "text";
}
