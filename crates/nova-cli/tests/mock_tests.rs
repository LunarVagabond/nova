use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;

/// Serializes the three tests below so only one `nova mock` child process
/// runs at a time. Running all three concurrently (default Rust test
/// parallelism within a binary) was found to intermittently starve one of
/// them out entirely on a resource-constrained CI runner — its connection
/// stayed refused for the *entire* retry deadline below rather than
/// succeeding partway through, which points to the process itself dying
/// (e.g. OOM) rather than just being slow to get scheduled. One at a time
/// removes that contention regardless of the exact cause.
static SERIAL: Mutex<()> = Mutex::new(());

fn temp_project_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "nova-cli-mock-tests-{name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn write_project(dir: &Path) {
    let nova_dir = dir.join("nova");
    fs::create_dir_all(nova_dir.join("collections")).unwrap();
    fs::create_dir_all(nova_dir.join("envs")).unwrap();

    fs::write(
        nova_dir.join("nova.yaml"),
        "version: 1\nproject:\n  name: mock-cli-test\n",
    )
    .unwrap();

    fs::write(
        nova_dir.join("envs/test.yaml"),
        "name: test\nvariables:\n  base_url: http://example.invalid\n",
    )
    .unwrap();

    fs::write(
        nova_dir.join("collections/hello.nova"),
        "[request]\nmethod: GET\nurl: {{base_url}}/hello\n\n[response 200]\nContent-Type: text/plain\n\nhi there\n",
    )
    .unwrap();

    fs::write(
        nova_dir.join("collections/missing.nova"),
        "[request]\nmethod: GET\nurl: {{base_url}}/missing\n",
    )
    .unwrap();

    fs::write(
        nova_dir.join("collections/multi.nova"),
        "[request]\nmethod: GET\nurl: {{base_url}}/multi\n\n[response 200 \"ok\"]\nContent-Type: text/plain\n\nfound\n\n[response 404 \"not_found\"]\nContent-Type: text/plain\n\nmissing\n",
    )
    .unwrap();
}

/// Spawns `nova mock` against `project_dir` on an OS-assigned port and
/// returns its base URL (read back from the process's own startup output)
/// along with the child handle to kill afterwards.
fn spawn_mock_server(project_dir: &Path) -> (String, std::process::Child) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_nova"))
        .args([
            "mock",
            project_dir.join("nova").to_str().unwrap(),
            "--port",
            "0",
        ])
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();

    let stdout = child.stdout.take().unwrap();
    let mut reader = BufReader::new(stdout);
    let mut first_line = String::new();
    reader.read_line(&mut first_line).unwrap();

    // "nova mock listening on http://127.0.0.1:PORT"
    let base_url = first_line
        .trim()
        .rsplit_once("http://")
        .map(|(_, addr)| format!("http://{addr}"))
        .expect("startup line should print the bound address");

    // Keep draining the rest of the banner (and anything else the process
    // ever writes) until the pipe closes on its own, rather than dropping
    // `reader` here and closing our read end out from under a still-
    // writing child — that used to make the child's own startup-banner
    // writes hit a broken pipe intermittently under CI load (see #196).
    // The banner no longer panics on that either way, but there's no
    // reason for the test to keep re-triggering the condition.
    std::thread::spawn(move || {
        let mut line = String::new();
        while reader.read_line(&mut line).unwrap_or(0) > 0 {
            line.clear();
        }
    });

    (base_url, child)
}

/// `nova mock` reports its bound address as soon as the socket is listening,
/// but under CI load the server process can take a while after that to
/// actually get scheduled and start accepting. Retry for several seconds
/// before treating a connection failure as a real failure, since it's
/// (usually) a startup race rather than a mock-server behavior bug — this
/// only costs time on the rare run that actually races.
///
/// If `child` has actually exited, though, no amount of retrying will ever
/// succeed — fail immediately with its exit status rather than waiting out
/// the full deadline to report a bare, uninformative "connection refused".
fn get(url: &str) -> ureq::RequestBuilder<ureq::typestate::WithoutBody> {
    // A non-2xx/3xx status (501, 404, ...) is exactly what several tests
    // below assert on — disable ureq's default of turning that into an
    // `Err` that discards the response body, so it comes back as an
    // ordinary `Response` these tests can inspect.
    ureq::get(url).config().http_status_as_error(false).build()
}

fn get_with_retry(url: &str, child: &mut Child) -> ureq::http::Response<ureq::Body> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        match get(url).call() {
            Err(source) if std::time::Instant::now() < deadline => {
                if let Ok(Some(status)) = child.try_wait() {
                    panic!("nova mock exited early with {status} instead of accepting a connection (last error: {source})");
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            result => {
                return result.unwrap_or_else(|source| panic!("request to {url} failed: {source}"))
            }
        }
    }
}

#[test]
fn serves_the_example_response_for_a_request_that_declares_one() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = temp_project_dir("example-response");
    write_project(&dir);

    let (base_url, mut child) = spawn_mock_server(&dir);

    let mut response = get_with_retry(&format!("{base_url}/hello"), &mut child);
    assert_eq!(response.status(), 200);
    assert_eq!(
        response.body_mut().read_to_string().unwrap().trim(),
        "hi there"
    );

    child.kill().unwrap();
    let _ = child.wait();
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn returns_501_for_a_registered_route_with_no_example_response() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = temp_project_dir("no-example");
    write_project(&dir);

    let (base_url, mut child) = spawn_mock_server(&dir);

    let response = get_with_retry(&format!("{base_url}/missing"), &mut child);
    assert_eq!(response.status(), 501);

    child.kill().unwrap();
    let _ = child.wait();
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn defaults_to_the_lowest_status_example_for_a_route_with_multiple() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = temp_project_dir("multi-default");
    write_project(&dir);

    let (base_url, mut child) = spawn_mock_server(&dir);

    let mut response = get_with_retry(&format!("{base_url}/multi"), &mut child);
    assert_eq!(response.status(), 200);
    assert_eq!(
        response.body_mut().read_to_string().unwrap().trim(),
        "found"
    );

    child.kill().unwrap();
    let _ = child.wait();
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn selects_an_example_by_name_header() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = temp_project_dir("multi-by-name");
    write_project(&dir);

    let (base_url, mut child) = spawn_mock_server(&dir);

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let mut response = loop {
        match get(&format!("{base_url}/multi"))
            .header("X-Nova-Mock-Example", "not_found")
            .call()
        {
            Err(_) if std::time::Instant::now() < deadline => {
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            result => break result.unwrap(),
        }
    };
    assert_eq!(response.status(), 404);
    assert_eq!(
        response.body_mut().read_to_string().unwrap().trim(),
        "missing"
    );

    child.kill().unwrap();
    let _ = child.wait();
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn selects_an_example_by_status_header() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = temp_project_dir("multi-by-status");
    write_project(&dir);

    let (base_url, mut child) = spawn_mock_server(&dir);

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let mut response = loop {
        match get(&format!("{base_url}/multi"))
            .header("X-Nova-Mock-Status", "404")
            .call()
        {
            Err(_) if std::time::Instant::now() < deadline => {
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            result => break result.unwrap(),
        }
    };
    assert_eq!(response.status(), 404);
    assert_eq!(
        response.body_mut().read_to_string().unwrap().trim(),
        "missing"
    );

    child.kill().unwrap();
    let _ = child.wait();
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn returns_404_for_a_path_with_no_matching_route() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = temp_project_dir("no-route");
    write_project(&dir);

    let (base_url, mut child) = spawn_mock_server(&dir);

    let response = get_with_retry(&format!("{base_url}/nope"), &mut child);
    assert_eq!(response.status(), 404);

    child.kill().unwrap();
    let _ = child.wait();
    fs::remove_dir_all(&dir).unwrap();
}
