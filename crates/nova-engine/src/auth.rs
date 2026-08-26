use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;

use crate::request::Header;

/// Base64-encode a friendly `Authorization: Basic {{username}}:{{password}}`
/// header (once variables are substituted) into the form HTTP Basic auth
/// actually requires on the wire.
///
/// Bearer tokens and API keys need no such transformation — they're just a
/// header (or, for an API key, sometimes a query param) whose value is
/// already what should go on the wire, so `{{variable}}` substitution alone
/// is enough for those. Basic auth is the one scheme with an encoding step
/// a request file shouldn't have to spell out by hand.
///
/// A header already free of a raw `:` is left untouched, on the assumption
/// it's already an encoded token rather than a literal `user:password`.
pub fn encode_basic_auth(headers: Vec<Header>) -> Vec<Header> {
    headers
        .into_iter()
        .map(|header| {
            if !header.name.eq_ignore_ascii_case("authorization") {
                return header;
            }

            let Some(rest) = header
                .value
                .strip_prefix("Basic ")
                .or_else(|| header.value.strip_prefix("basic "))
            else {
                return header;
            };
            let rest = rest.trim();

            if !rest.contains(':') {
                return header;
            }

            Header {
                name: header.name,
                value: format!("Basic {}", BASE64.encode(rest.as_bytes())),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_a_raw_user_password_pair() {
        let headers = vec![Header {
            name: "Authorization".to_string(),
            value: "Basic developer:hunter2".to_string(),
        }];

        let result = encode_basic_auth(headers);

        assert_eq!(result[0].value, "Basic ZGV2ZWxvcGVyOmh1bnRlcjI=");
    }

    #[test]
    fn leaves_an_already_encoded_token_alone() {
        let headers = vec![Header {
            name: "Authorization".to_string(),
            value: "Basic ZGV2ZWxvcGVyOmh1bnRlcjI=".to_string(),
        }];

        let result = encode_basic_auth(headers.clone());

        assert_eq!(result[0].value, headers[0].value);
    }

    #[test]
    fn leaves_bearer_and_other_headers_untouched() {
        let headers = vec![
            Header {
                name: "Authorization".to_string(),
                value: "Bearer some-token".to_string(),
            },
            Header {
                name: "X-Api-Key".to_string(),
                value: "some-key".to_string(),
            },
        ];

        let result = encode_basic_auth(headers.clone());

        assert_eq!(result, headers);
    }
}
