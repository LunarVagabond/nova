use std::sync::mpsc;
use std::thread;

use nova_engine::{ParsedRequest, RequestBody, Session};

fn cookie_header_of(request: &tiny_http::Request) -> Option<String> {
    request
        .headers()
        .iter()
        .find(|h| h.field.as_str().as_str().eq_ignore_ascii_case("cookie"))
        .map(|h| h.value.as_str().to_string())
}

#[test]
fn persists_cookies_across_requests_in_the_same_session() {
    let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
    let addr = server.server_addr();
    let url = format!("http://{addr}");
    let (tx, rx) = mpsc::channel();

    let handle = thread::spawn(move || {
        let login = server.recv().unwrap();
        login
            .respond(
                tiny_http::Response::from_string("logged in").with_header(
                    tiny_http::Header::from_bytes(
                        &b"Set-Cookie"[..],
                        &b"session_id=abc123; Path=/"[..],
                    )
                    .unwrap(),
                ),
            )
            .unwrap();

        let me = server.recv().unwrap();
        tx.send(cookie_header_of(&me)).unwrap();
        me.respond(tiny_http::Response::from_string("me")).unwrap();
    });

    let mut session = Session::new();

    let login_request = ParsedRequest {
        method: "GET".to_string(),
        url: format!("{url}/login"),
        headers: vec![],
        body: RequestBody::None,
    };
    session.execute(&login_request).unwrap();

    let me_request = ParsedRequest {
        method: "GET".to_string(),
        url: format!("{url}/me"),
        headers: vec![],
        body: RequestBody::None,
    };
    session.execute(&me_request).unwrap();

    handle.join().unwrap();

    assert_eq!(rx.recv().unwrap(), Some("session_id=abc123".to_string()));
}

#[test]
fn a_fresh_session_does_not_carry_cookies_from_another_session() {
    let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
    let addr = server.server_addr();
    let url = format!("http://{addr}");
    let (tx, rx) = mpsc::channel();

    let handle = thread::spawn(move || {
        // First session's login, which sets a cookie.
        let login = server.recv().unwrap();
        login
            .respond(
                tiny_http::Response::from_string("logged in").with_header(
                    tiny_http::Header::from_bytes(&b"Set-Cookie"[..], &b"session_id=abc123"[..])
                        .unwrap(),
                ),
            )
            .unwrap();

        // A second, unrelated session's request to the same host — should
        // arrive with no Cookie header, since its jar was never told about
        // the first session's cookie.
        let check = server.recv().unwrap();
        tx.send(cookie_header_of(&check)).unwrap();
        check
            .respond(tiny_http::Response::from_string("ok"))
            .unwrap();
    });

    let mut first_session = Session::new();
    let login_request = ParsedRequest {
        method: "GET".to_string(),
        url: format!("{url}/login"),
        headers: vec![],
        body: RequestBody::None,
    };
    first_session.execute(&login_request).unwrap();

    let mut second_session = Session::new();
    let check_request = ParsedRequest {
        method: "GET".to_string(),
        url: format!("{url}/check"),
        headers: vec![],
        body: RequestBody::None,
    };
    second_session.execute(&check_request).unwrap();

    handle.join().unwrap();

    assert_eq!(rx.recv().unwrap(), None);
}
