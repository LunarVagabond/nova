#!/usr/bin/env node
// Post-response test fixture script: reads the response JSON (status,
// headers, body, elapsed_ms) from stdin and hands back the variables it
// extracted, per the post-response JSON contract described in
// nova-engine's `script` module.

let raw = "";
process.stdin.on("data", (chunk) => {
  raw += chunk;
});
process.stdin.on("end", () => {
  const response = JSON.parse(raw);
  const body = JSON.parse(response.body);
  console.log(JSON.stringify({ token: body.token }));
});
