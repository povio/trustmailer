import wasmModule from "../pkg/trustmailer_smtp_cfworkers_bg.wasm";
import { initSync, createTransport } from "../pkg/trustmailer_smtp_cfworkers.js";

let initError = null;
try {
  initSync({ module: wasmModule });
} catch (e) {
  initError = String(e && e.stack ? e.stack : e);
}

export default {
  async fetch(request) {
    const url = new URL(request.url);

    if (url.searchParams.has("debug")) {
      return Response.json({
        typeofWasmModule: typeof wasmModule,
        wasmModuleIsWebAssemblyModule: wasmModule instanceof WebAssembly.Module,
        typeofInitSync: typeof initSync,
        typeofCreateTransport: typeof createTransport,
        initError,
      });
    }

    const host = url.searchParams.get("host") || "127.0.0.1";
    const port = Number(url.searchParams.get("port") || "2525");

    try {
      const transporter = createTransport({
        host,
        port,
        secure: false,
        requireTLS: false, // local fake SMTP server has no TLS support
        name: "cfworkers-test.local",
      });

      const info = await transporter.sendMail({
        from: "Alice <alice@example.com>",
        to: "bob@example.com",
        subject: "Hello from a real Cloudflare Worker (workerd)",
        text: "Sent over a genuine cloudflare:sockets TCP connection, not an HTTP relay.",
      });

      return Response.json({ ok: true, info });
    } catch (err) {
      return Response.json({ ok: false, error: String(err && err.stack ? err.stack : err) }, { status: 500 });
    }
  },
};
