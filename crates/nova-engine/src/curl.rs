//! Parsing a copy-pasted `curl`/`wget` command line into the pieces of a
//! request — used by the GUI's "paste a curl command into the URL field"
//! convenience (see `nova-app`'s `RequestPanel.vue`). This never touches
//! `.nova` file syntax itself; the caller takes the parsed pieces and
//! drives whatever request-editing state it already has, exactly like it
//! would for manual edits.

use serde::Serialize;

use crate::request::Header;

/// The pieces of a request recovered from a `curl`/`wget` command line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ParsedCurlRequest {
    pub method: String,
    pub url: String,
    pub headers: Vec<Header>,
    /// Concatenated body text (multiple `-d`/`--data` flags are joined with
    /// `&`, matching curl's own behavior), or `None` if the command has no
    /// body at all.
    pub body: Option<String>,
}

/// Parse a `curl` or `wget` command line into its method/URL/headers/body.
/// Recognizes the common flags people actually paste (from browser dev
/// tools' "Copy as cURL", docs, or their own shell history) — not a full
/// reimplementation of curl's option surface. Flags with no Nova
/// equivalent (`-k`/`--insecure`, `-v`/`--verbose`, `-L`/`--location`,
/// `--compressed`, etc.) are silently accepted and ignored rather than
/// rejected, so an otherwise-ordinary pasted command doesn't fail to parse
/// over an option Nova has no use for.
pub fn parse_curl(command: &str) -> Result<ParsedCurlRequest, String> {
    let tokens = tokenize(command)?;
    let mut tokens = tokens.into_iter().peekable();

    match tokens.next().as_deref() {
        Some("curl") | Some("wget") => {}
        Some(other) => return Err(format!("expected a curl or wget command, found {other:?}")),
        None => return Err("empty command".to_string()),
    }

    let mut method: Option<String> = None;
    let mut url: Option<String> = None;
    let mut headers: Vec<Header> = Vec::new();
    let mut body_parts: Vec<String> = Vec::new();
    let mut is_head = false;

    // Flags that take a value as the next token.
    const VALUE_FLAGS: &[&str] = &[
        "-X",
        "--request",
        "-H",
        "--header",
        "-d",
        "--data",
        "--data-raw",
        "--data-binary",
        "--data-urlencode",
        "-u",
        "--user",
        "-A",
        "--user-agent",
        "-b",
        "--cookie",
        "-e",
        "--referer",
        "-o",
        "--output",
        "--url",
    ];
    // Flags with no Nova equivalent and no value — accepted and ignored.
    const IGNORED_FLAGS: &[&str] = &[
        "-k",
        "--insecure",
        "-v",
        "--verbose",
        "-s",
        "--silent",
        "-S",
        "--show-error",
        "-L",
        "--location",
        "--compressed",
        "-i",
        "--include",
        "-#",
        "--progress-bar",
        "-f",
        "--fail",
        "-N",
        "--no-buffer",
    ];

    while let Some(token) = tokens.next() {
        if token == "-I" || token == "--head" {
            is_head = true;
            continue;
        }
        if token == "-G" || token == "--get" {
            method = Some("GET".to_string());
            continue;
        }
        if IGNORED_FLAGS.contains(&token.as_str()) {
            continue;
        }
        if VALUE_FLAGS.contains(&token.as_str()) {
            let value = tokens
                .next()
                .ok_or_else(|| format!("{token} is missing its value"))?;
            apply_flag(
                &token,
                &value,
                &mut method,
                &mut url,
                &mut headers,
                &mut body_parts,
            )?;
            continue;
        }
        // `-XPOST`/`-H"..."` style: a short flag glued directly to its
        // value with no separating space.
        if let Some(rest) = token.strip_prefix("-X") {
            if !rest.is_empty() {
                apply_flag(
                    "-X",
                    rest,
                    &mut method,
                    &mut url,
                    &mut headers,
                    &mut body_parts,
                )?;
                continue;
            }
        }
        if token.starts_with('-') {
            // An option this parser doesn't recognize — ignore it rather
            // than failing the whole paste over one unusual flag, the same
            // best-effort spirit as the rest of Nova's converters.
            continue;
        }

        // A bare token that isn't a flag or a flag's value: the URL, the
        // first time we see one.
        if url.is_none() {
            url = Some(token);
        }
    }

    let url = url.ok_or_else(|| "no URL found in curl command".to_string())?;
    let body = if body_parts.is_empty() {
        None
    } else {
        Some(body_parts.join("&"))
    };

    let method = if is_head {
        "HEAD".to_string()
    } else if let Some(method) = method {
        method
    } else if body.is_some() {
        // curl itself defaults to POST when there's a body and no explicit
        // method.
        "POST".to_string()
    } else {
        "GET".to_string()
    };

    Ok(ParsedCurlRequest {
        method: method.to_ascii_uppercase(),
        url,
        headers,
        body,
    })
}

fn apply_flag(
    flag: &str,
    value: &str,
    method: &mut Option<String>,
    url: &mut Option<String>,
    headers: &mut Vec<Header>,
    body_parts: &mut Vec<String>,
) -> Result<(), String> {
    match flag {
        "-X" | "--request" => *method = Some(value.to_string()),
        "--url" => *url = Some(value.to_string()),
        "-H" | "--header" => {
            let (name, header_value) = value.split_once(':').ok_or_else(|| {
                format!("malformed -H value (expected \"Name: value\"): {value:?}")
            })?;
            headers.push(Header {
                name: name.trim().to_string(),
                value: header_value.trim().to_string(),
            });
        }
        "-d" | "--data" | "--data-raw" | "--data-binary" | "--data-urlencode" => {
            body_parts.push(value.to_string());
        }
        "-u" | "--user" => {
            use base64::Engine;
            let encoded = base64::engine::general_purpose::STANDARD.encode(value);
            headers.push(Header {
                name: "Authorization".to_string(),
                value: format!("Basic {encoded}"),
            });
        }
        "-A" | "--user-agent" => headers.push(Header {
            name: "User-Agent".to_string(),
            value: value.to_string(),
        }),
        "-b" | "--cookie" => headers.push(Header {
            name: "Cookie".to_string(),
            value: value.to_string(),
        }),
        "-e" | "--referer" => headers.push(Header {
            name: "Referer".to_string(),
            value: value.to_string(),
        }),
        // -o/--output names a file to write the response to — no bearing
        // on the request itself, so just consume its value and move on.
        "-o" | "--output" => {}
        _ => {}
    }
    Ok(())
}

/// Shell-like tokenization: splits on whitespace, respects single quotes
/// (fully literal, per POSIX shell rules) and double quotes (backslash
/// escapes recognized inside), and treats a trailing `\` at end-of-line as
/// a line continuation — the shape browser "Copy as cURL" output and
/// hand-typed multi-line commands both actually use.
fn tokenize(command: &str) -> Result<Vec<String>, String> {
    let joined = command.replace("\\\n", " ").replace("\\\r\n", " ");

    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut has_current = false;
    let mut chars = joined.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            c if c.is_whitespace() => {
                if has_current {
                    tokens.push(std::mem::take(&mut current));
                    has_current = false;
                }
            }
            '\'' => {
                has_current = true;
                for c in chars.by_ref() {
                    if c == '\'' {
                        break;
                    }
                    current.push(c);
                }
            }
            '"' => {
                has_current = true;
                loop {
                    match chars.next() {
                        Some('"') => break,
                        Some('\\') => {
                            if let Some(escaped) = chars.next() {
                                current.push(escaped);
                            }
                        }
                        Some(c) => current.push(c),
                        None => return Err("unterminated \" quote".to_string()),
                    }
                }
            }
            '\\' => {
                has_current = true;
                if let Some(escaped) = chars.next() {
                    current.push(escaped);
                }
            }
            c => {
                has_current = true;
                current.push(c);
            }
        }
    }
    if has_current {
        tokens.push(current);
    }

    Ok(tokens)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_simple_get() {
        let parsed = parse_curl("curl https://api.example.com/users").unwrap();
        assert_eq!(parsed.method, "GET");
        assert_eq!(parsed.url, "https://api.example.com/users");
        assert!(parsed.headers.is_empty());
        assert_eq!(parsed.body, None);
    }

    #[test]
    fn parses_method_headers_and_body() {
        let parsed = parse_curl(
            r#"curl -X POST https://api.example.com/login -H "Content-Type: application/json" -H "Accept: application/json" -d '{"username":"a","password":"b"}'"#,
        )
        .unwrap();
        assert_eq!(parsed.method, "POST");
        assert_eq!(parsed.url, "https://api.example.com/login");
        assert_eq!(
            parsed.headers,
            vec![
                Header {
                    name: "Content-Type".to_string(),
                    value: "application/json".to_string()
                },
                Header {
                    name: "Accept".to_string(),
                    value: "application/json".to_string()
                },
            ]
        );
        assert_eq!(
            parsed.body,
            Some(r#"{"username":"a","password":"b"}"#.to_string())
        );
    }

    #[test]
    fn a_body_with_no_explicit_method_defaults_to_post() {
        let parsed = parse_curl("curl https://api.example.com/x -d 'a=1'").unwrap();
        assert_eq!(parsed.method, "POST");
    }

    #[test]
    fn multiple_data_flags_are_joined_with_ampersand() {
        let parsed = parse_curl("curl https://api.example.com/x -d 'a=1' -d 'b=2'").unwrap();
        assert_eq!(parsed.body, Some("a=1&b=2".to_string()));
    }

    #[test]
    fn glued_short_method_flag_is_recognized() {
        let parsed = parse_curl("curl -XDELETE https://api.example.com/x").unwrap();
        assert_eq!(parsed.method, "DELETE");
    }

    #[test]
    fn head_flag_sets_method() {
        let parsed = parse_curl("curl -I https://api.example.com/x").unwrap();
        assert_eq!(parsed.method, "HEAD");
    }

    #[test]
    fn basic_auth_becomes_an_authorization_header() {
        let parsed = parse_curl("curl -u alice:secret https://api.example.com/x").unwrap();
        assert_eq!(
            parsed.headers,
            vec![Header {
                name: "Authorization".to_string(),
                value: "Basic YWxpY2U6c2VjcmV0".to_string(),
            }]
        );
    }

    #[test]
    fn unrecognized_flags_are_ignored_rather_than_rejected() {
        let parsed = parse_curl("curl -sS -L --compressed -k https://api.example.com/x").unwrap();
        assert_eq!(parsed.url, "https://api.example.com/x");
        assert_eq!(parsed.method, "GET");
    }

    #[test]
    fn wget_prefix_is_accepted() {
        let parsed = parse_curl("wget https://api.example.com/x").unwrap();
        assert_eq!(parsed.url, "https://api.example.com/x");
    }

    #[test]
    fn line_continuations_are_joined() {
        let parsed = parse_curl(
            "curl https://api.example.com/x \\\n  -H 'Accept: application/json' \\\n  -d 'a=1'",
        )
        .unwrap();
        assert_eq!(parsed.url, "https://api.example.com/x");
        assert_eq!(
            parsed.headers,
            vec![Header {
                name: "Accept".to_string(),
                value: "application/json".to_string()
            }]
        );
        assert_eq!(parsed.body, Some("a=1".to_string()));
    }

    #[test]
    fn not_a_curl_command_is_a_typed_error() {
        let err = parse_curl("echo hello").unwrap_err();
        assert!(err.contains("curl or wget"), "unexpected error: {err}");
    }

    #[test]
    fn empty_command_is_a_typed_error() {
        assert!(parse_curl("").is_err());
    }
}
