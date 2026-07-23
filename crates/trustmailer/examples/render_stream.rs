//! Renders a fixed message to raw MIME via `trustmailer-stream` and prints it to
//! stdout, for `benchmarks/parity.cjs`'s CRLF-injection regression check — JSON
//! transport doesn't build real headers (see the comparison page), so the header-
//! injection defense has to be checked against a transport that actually does.

use trustmailer::stream::StreamTransport;
use trustmailer::Message;

fn main() {
    let message = Message::builder()
        .from("alice@example.com")
        .to("bob@example.com")
        .subject("Hello\r\nX-Injected: evil")
        .text("hi")
        .build();

    let transport = StreamTransport::new();
    let raw = transport.render(&message).expect("render should not fail");
    print!("{}", String::from_utf8_lossy(&raw));
}
