//! Reading what servers say: dates, waits, validators, checksums.

use super::*;

const ABC_SHA256: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

#[test]
fn reads_http_dates() {
    assert_eq!(http_date("Sun, 06 Nov 1994 08:49:37 GMT"), Some(784_111_777));
    assert_eq!(http_date("Thu, 01 Jan 1970 00:00:00 GMT"), Some(0));
    assert_eq!(http_date("Sat, 29 Feb 2020 12:00:00 GMT"), Some(1_582_977_600));
    assert_eq!(http_date("yesterday"), None);
    assert_eq!(http_date("Sun, 06 Foo 1994 08:49:37 GMT"), None);
}

#[test]
fn retry_after_in_seconds_or_as_a_date() {
    assert_eq!(retry_after(" 120 "), Some(Duration::from_secs(120)));
    assert_eq!(retry_after("100000"), Some(LONGEST_WAIT));
    assert_eq!(retry_after("Sun, 06 Nov 1994 08:49:37 GMT"), Some(Duration::ZERO)); // (past)
    assert_eq!(retry_after("soon"), None);
}

#[test]
fn only_strong_validators_count() {
    let date = Some("Sun, 06 Nov 1994 09:00:00 GMT");
    let v = Validators::from_headers(Some("W/\"abc\""), Some("Sun, 06 Nov 1994 08:49:37 GMT"), date);
    assert_eq!((v.etag, v.modified.is_some()), (None, true));
    // modified the same second it was sent: may change again within it
    let v = Validators::from_headers(None, Some("Sun, 06 Nov 1994 09:00:00 GMT"), date);
    assert_eq!(v.modified, None);
}

#[test]
fn tells_a_changed_file() {
    let a = Validators { etag: Some("\"1\"".into()), modified: Some("x".into()) };
    let b = Validators { etag: Some("\"2\"".into()), modified: Some("x".into()) };
    assert!(a.changed(&b));
    assert!(!a.changed(&a));
    assert!(!a.changed(&Validators::default())); // (can't tell)
    let old = Validators { etag: None, modified: Some("Mon".into()) };
    assert!(old.changed(&Validators { etag: None, modified: Some("Tue".into()) }));
}

#[test]
fn parses_checksums() {
    let c = Checksum::parse(&ABC_SHA256.to_uppercase()).unwrap();
    assert_eq!((c.algorithm, c.hex.as_str()), (Algorithm::Sha256, ABC_SHA256));
    assert_eq!(Checksum::parse(&format!("sha-256:{ABC_SHA256}")), Some(c.clone()));
    assert_eq!(Checksum::parse(&format!("sha512:{ABC_SHA256}")), None);
    assert_eq!(Checksum::parse("abc"), None);
    let header = "sha-256=:ungWv48Bz+pBQUDeXa4iI7ADYaOWF3qctBD/YfIAFa0=:";
    assert_eq!(Checksum::from_headers(Some(header), None), Some(c.clone()));
    let old = "MD5=kAFQmDzST7DWlj99KOF/cg==, SHA-256=ungWv48Bz+pBQUDeXa4iI7ADYaOWF3qctBD/YfIAFa0=";
    assert_eq!(Checksum::from_headers(None, Some(old)), Some(c));
    assert_eq!(Checksum::from_headers(None, Some("MD5=kAFQmDzST7DWlj99KOF/cg==")), None);
}

#[test]
fn verifies_a_file() {
    let path = std::env::temp_dir().join(format!("turbodm-check-{}", std::process::id()));
    std::fs::write(&path, b"abc").unwrap();
    let good = Checksum::parse(ABC_SHA256).unwrap();
    assert!(good.verify(&path, 3, |_| {}).is_ok());
    let bad = Checksum::parse(&"0".repeat(64)).unwrap();
    assert!(bad.verify(&path, 3, |_| {}).unwrap_err().contains("damaged"));
    let _ = std::fs::remove_file(path);
}
