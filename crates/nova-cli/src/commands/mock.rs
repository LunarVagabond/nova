use std::io::{Cursor, Write};
use std::path::Path;

use nova_engine::{mock_routes, MockRoute, NovaProject};

/// Discover a project's requests and start a local HTTP server that
/// answers each one's route with its declared example response.
///
/// Prints the bound address and the full route table before serving, then
/// blocks handling requests until the process is killed. This is a static
/// mock: nothing a mocked request does affects the response of a later
/// mocked request.
pub fn run(path: &Path, host: &str, port: u16) -> Result<(), String> {
    let project = NovaProject::discover(path).map_err(|e| e.to_string())?;
    let routes = mock_routes(&project).map_err(|e| e.to_string())?;

    let server = tiny_http::Server::http((host, port))
        .map_err(|e| format!("failed to bind {host}:{port}: {e}"))?;
    let addr = server.server_addr();

    println!("nova mock listening on http://{addr}");
    println!();
    if routes.is_empty() {
        println!("(no requests found in this project)");
    } else {
        println!("routes:");
        for route in &routes {
            let status = route
                .example_response
                .as_ref()
                .map(|response| response.status.to_string())
                .unwrap_or_else(|| "501 (no example response)".to_string());
            println!("  {:<7} {:<40} -> {status}", route.method, route.path);
        }
    }
    println!();
    std::io::stdout().flush().map_err(|e| e.to_string())?;

    for request in server.incoming_requests() {
        handle_request(request, &routes);
    }

    Ok(())
}

fn handle_request(request: tiny_http::Request, routes: &[MockRoute]) {
    let method = request.method().to_string();
    let full_path = request.url().to_string();
    let path = full_path.split('?').next().unwrap_or("/");

    let matched = routes.iter().find(|route| route.matches(&method, path));
    let response = build_response(matched, &method, path);

    let _ = request.respond(response);
}

fn build_response(
    matched: Option<&MockRoute>,
    method: &str,
    path: &str,
) -> tiny_http::Response<Cursor<Vec<u8>>> {
    let Some(route) = matched else {
        return tiny_http::Response::from_string(format!(
            "no route registered for {method} {path}\n"
        ))
        .with_status_code(404);
    };

    let Some(example) = &route.example_response else {
        return tiny_http::Response::from_string(format!(
            "no example response defined for {} {} — add a \"### response\" section to {}\n",
            route.method,
            route.path,
            route.source.display()
        ))
        .with_status_code(501);
    };

    let mut response =
        tiny_http::Response::from_string(example.body.clone()).with_status_code(example.status);
    for header in &example.headers {
        if let Ok(header) =
            tiny_http::Header::from_bytes(header.name.as_bytes(), header.value.as_bytes())
        {
            response = response.with_header(header);
        }
    }
    response
}
