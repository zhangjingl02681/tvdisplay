use std::io::{self, Write};
use tracing::level_filters::LevelFilter;
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{fmt, Registry};

pub const REDACTED: &str = "***";

#[derive(Clone, Debug)]
pub struct Secrets(pub Vec<String>);

pub fn redact(text: &str, secrets: &[String]) -> String {
    let mut output = text.to_string();
    for secret in secrets {
        if secret.is_empty() {
            continue;
        }
        output = output.replace(secret, REDACTED);
    }
    output
}

pub fn install_redaction(secrets: &[String]) {
    install_redaction_with_writer(secrets, io::stderr);
}

pub fn install_redaction_with_writer<W>(secrets: &[String], writer: W)
where
    W: for<'a> MakeWriter<'a> + Send + Sync + 'static,
{
    let secrets = secrets
        .iter()
        .filter(|secret| !secret.is_empty())
        .cloned()
        .collect::<Vec<_>>();
    let writer = RedactingWriter {
        secrets: secrets.clone(),
        inner: writer,
    };
    let _ = Registry::default()
        .with(LevelFilter::INFO)
        .with(fmt::layer().with_writer(writer).with_target(false))
        .try_init();
}

struct RedactingWriter<W> {
    secrets: Vec<String>,
    inner: W,
}

impl<'a, W> MakeWriter<'a> for RedactingWriter<W>
where
    W: for<'b> MakeWriter<'b> + Send + Sync + 'static,
{
    type Writer = RedactingIo<<W as MakeWriter<'a>>::Writer>;

    fn make_writer(&'a self) -> Self::Writer {
        RedactingIo {
            secrets: self.secrets.clone(),
            inner: self.inner.make_writer(),
        }
    }
}

struct RedactingIo<W> {
    secrets: Vec<String>,
    inner: W,
}

impl<W: Write> Write for RedactingIo<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let text = String::from_utf8_lossy(buf);
        let redacted = redact(&text, &self.secrets);
        self.inner.write_all(redacted.as_bytes())?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}
