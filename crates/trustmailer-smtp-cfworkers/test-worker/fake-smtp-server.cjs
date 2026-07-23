// Standalone plaintext fake SMTP server for testing the Cloudflare Worker SMTP
// transport end-to-end via `wrangler dev` (which can reach 127.0.0.1 on the host).
// Mirrors the behavior of crates/trustmailer-smtp/tests/fake_smtp.rs.
const net = require("net");

const port = Number(process.argv[2] || 2525);

const server = net.createServer((socket) => {
  let buffer = "";
  const lines = [];
  socket.on("error", (err) => console.log("(socket error, ignored)", err.code || err.message));
  socket.write("220 fake.smtp ESMTP\r\n");

  socket.on("data", (chunk) => {
    buffer += chunk.toString("utf8");
    let idx;
    while ((idx = buffer.indexOf("\r\n")) !== -1) {
      const line = buffer.slice(0, idx);
      buffer = buffer.slice(idx + 2);
      lines.push(line);
      console.log("<-", line);
      handleLine(socket, line);
    }
  });

  let inData = false;
  let dataBuf = [];

  function handleLine(socket, line) {
    const upper = line.toUpperCase();
    if (inData) {
      if (line === ".") {
        inData = false;
        console.log("--- end of DATA, body was:\n" + dataBuf.join("\n") + "\n---");
        write(socket, "250 OK: queued as 12345");
      } else {
        dataBuf.push(line);
      }
      return;
    }
    if (upper.startsWith("EHLO")) {
      write(socket, "250-fake.smtp\r\n250 PIPELINING");
    } else if (upper.startsWith("MAIL FROM")) {
      write(socket, "250 OK");
    } else if (upper.startsWith("RCPT TO")) {
      write(socket, "250 OK");
    } else if (upper.startsWith("DATA")) {
      dataBuf = [];
      inData = true;
      write(socket, "354 Start mail input");
    } else if (upper.startsWith("NOOP")) {
      write(socket, "250 OK");
    } else if (upper.startsWith("QUIT")) {
      write(socket, "221 Bye");
      socket.end();
    } else {
      write(socket, "500 unrecognized command");
    }
  }

  function write(socket, text) {
    console.log("->", text.replace(/\r\n/g, "\\r\\n|"));
    socket.write(text + "\r\n");
  }
});

server.listen(port, "127.0.0.1", () => {
  console.log(`fake SMTP server listening on 127.0.0.1:${port}`);
});
