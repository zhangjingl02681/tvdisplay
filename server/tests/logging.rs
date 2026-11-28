use std::io::Write;
use std::sync::{Arc, Mutex};
#[allow(unused_imports)]
use tvdisplay::logging::{install_redaction, install_redaction_with_writer, redact, REDACTED};

#[test]
fn redact_replaces_every_secret() {
    let secrets = vec!["unit-test-password".into(), "feishu-app-secret".into()];
    let rendered = redact(
        "rejected unit-test-password feishu-app-secret",
        &secrets,
    );
    assert!(!rendered.contains("unit-test-password"));
    assert!(!rendered.contains("feishu-app-secret"));
    assert!(rendered.contains(REDACTED));
}

#[test]
fn empty_secret_is_ignored() {
    assert_eq!(redact("hello", &["".into()]), "hello");
}

#[test]
fn install_redaction_filters_tracing_output() {
    let password = "unit-test-password".to_string();
    let buf = std::sync::Arc::new(std::sync::Mutex::new(Vec::<u8>::new()));
    let writer = Writer(buf.clone());
    install_redaction_with_writer(&[password.clone()], writer);
    tracing::info!("rejected {}", password);
    let rendered = String::from_utf8(buf.lock().unwrap().clone()).unwrap();
    assert!(!rendered.contains(&password));
    assert!(rendered.contains("***"));
}

#[derive(Clone)]
struct Writer(Arc<Mutex<Vec<u8>>>);

impl Write for Writer {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Writer {
    type Writer = Writer;
    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}
