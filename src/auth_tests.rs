use super::*;

#[test]
fn challenge_matches_rfc_7636_example() {
    assert_eq!(challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"), "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
}

#[test]
fn verifier_is_long_enough_and_unique() {
    let (a, b) = (Pkce::new(), Pkce::new());
    assert!((43..=128).contains(&a.verifier.len()));
    assert_ne!(a.verifier, b.verifier);
    assert_eq!(a.challenge, challenge(&a.verifier));
}

#[test]
fn callback_code_is_read() {
    let result = parse_callback("GET /callback?code=abc-123 HTTP/1.1\r\n");
    assert_eq!(result.unwrap().unwrap(), "abc-123");
}

#[test]
fn callback_error_is_reported() {
    let result = parse_callback("GET /callback?error=access_denied&error_description=User+denied%20access HTTP/1.1");
    let message = result.unwrap().unwrap_err().to_string();
    assert!(message.contains("User denied access"), "{message}");
}

#[test]
fn callback_without_code_is_an_error() {
    assert!(parse_callback("GET /callback HTTP/1.1").unwrap().is_err());
}

#[test]
fn other_requests_are_ignored() {
    assert!(parse_callback("GET /favicon.ico HTTP/1.1").is_none());
    assert!(parse_callback("POST /callback?code=x HTTP/1.1").is_none());
    assert!(parse_callback("").is_none());
}

#[test]
fn dpapi_round_trip() {
    let sealed = protect(b"refresh-token").unwrap();
    assert_ne!(sealed, b"refresh-token");
    assert_eq!(unprotect(&sealed).unwrap(), b"refresh-token");
}

#[test]
fn loopback_waits_for_the_callback_and_honours_cancel() {
    use std::io::Read;
    let cancelled = AtomicBool::new(true);
    assert!(matches!(Loopback::bind().unwrap().wait_for_code(&cancelled), Err(Error::Cancelled)));

    let listener = Loopback::bind().unwrap();
    let browser = thread::spawn(|| {
        ["/favicon.ico", "/callback?code=xyz"].map(|path| {
            let mut stream = TcpStream::connect(("127.0.0.1", PORT)).unwrap();
            write!(stream, "GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n").unwrap();
            let mut reply = String::new();
            stream.read_to_string(&mut reply).unwrap();
            reply
        })
    });
    assert_eq!(listener.wait_for_code(&AtomicBool::new(false)).unwrap(), "xyz");
    let [favicon, callback] = browser.join().unwrap();
    assert!(favicon.starts_with("HTTP/1.1 404"));
    assert!(callback.starts_with("HTTP/1.1 200") && callback.contains("Giriş tamamlandı"));
}
