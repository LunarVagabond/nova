// Pure helpers for interpreting/driving a request's Body tab — kept out of
// RequestPanel.vue so this logic can be unit tested and read on its own,
// independent of Vue reactivity.
import type { RequestHeader } from "../types/nova";
import type { EditorLanguage } from "../components/CodeEditor.vue";

export type BodyType = "none" | "json" | "xml" | "form" | "multipart" | "text";

export const BODY_TYPE_OPTIONS: BodyType[] = ["none", "json", "xml", "form", "multipart", "text"];

export const BODY_TYPE_LABELS: Record<BodyType, string> = {
  none: "No Body",
  json: "JSON",
  xml: "XML",
  form: "Form URL Encoded",
  multipart: "Multipart Form Data",
  text: "Plain Text",
};

export const BODY_TYPE_CONTENT_TYPES: Record<Exclude<BodyType, "none">, string> = {
  json: "application/json",
  xml: "application/xml",
  form: "application/x-www-form-urlencoded",
  multipart: "multipart/form-data",
  text: "text/plain",
};

/** Infers a body type from the Content-Type header (and whether there's any body text at all). */
export function detectBodyType(currentHeaders: RequestHeader[], text: string): BodyType {
  if (text.trim() === "") return "none";
  const contentType = currentHeaders.find((h) => h.name.toLowerCase() === "content-type")?.value ?? "";
  const essence = contentType.split(";")[0]?.trim().toLowerCase() ?? "";
  if (essence === "application/json" || essence.endsWith("+json")) return "json";
  if (essence === "application/xml" || essence === "text/xml" || essence.endsWith("+xml")) return "xml";
  if (essence === "application/x-www-form-urlencoded") return "form";
  if (essence === "multipart/form-data") return "multipart";
  return "text";
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
  return "text";
}
