//! Renders a fixed message to JSON via `trustmailer-json` and prints it to stdout.
//! Used by `benchmarks/parity.cjs` to compare trustmailer's real output against
//! nodemailer's real `jsonTransport` output for the same logical input — not a
//! standalone demo, a reproducible comparison fixture.

use trustmailer::json::JsonTransport;
use trustmailer::Message;

fn main() {
    let mode = std::env::args().nth(1).unwrap_or_default();

    let message = if mode == "injection" {
        Message::builder()
            .from("alice@example.com")
            .to("bob@example.com")
            .subject("Hello\r\nX-Injected: evil")
            .text("hi")
            .build()
    } else {
        Message::builder()
            .from("Alice <alice@example.com>")
            .to("Bob <bob@example.com>, carol@example.com")
            .cc("dave@example.com")
            .subject("Parity check")
            .text("Hi Bob!")
            .html("<b>Hi Bob!</b>")
            .build()
    };

    let transport = JsonTransport::new();
    let json = transport.render(&message).expect("render should not fail");
    println!("{json}");
}
