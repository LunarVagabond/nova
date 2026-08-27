// URLSearchParams-based encode/decode shared by the URL bar's query-string
// sync and the Body tab's `form` (application/x-www-form-urlencoded)
// structured editor — both are the same wire format applied to a different
// field.
import type { QueryParam } from "../types/nova";

export function serializeQuery(params: QueryParam[]): string {
  const usp = new URLSearchParams();
  for (const param of params) {
    if (param.name === "" && param.value === "") continue;
    usp.append(param.name, param.value);
  }
  return usp.toString();
}

export function parseQueryString(queryString: string): QueryParam[] {
  return Array.from(new URLSearchParams(queryString).entries()).map(([name, value]) => ({
    name,
    value,
  }));
}

export function splitUrlAndQuery(raw: string): { base: string; query: QueryParam[] } {
  const index = raw.indexOf("?");
  if (index === -1) return { base: raw, query: [] };
  return { base: raw.slice(0, index), query: parseQueryString(raw.slice(index + 1)) };
}
