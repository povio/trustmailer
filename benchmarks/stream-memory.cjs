// Peak RSS of one EML download with large attachments, buffered vs streamed.
// Run each mode in a fresh process:
//   node benchmarks/stream-memory.cjs buffer 10 10
//   node benchmarks/stream-memory.cjs stream 10 10   (or bun ...)
// Each attachment comes from a source yielding 64 KiB chunks, standing in for an S3
// object stream. `buffer` reads every attachment into a Buffer and calls renderEml,
// as callers had to before renderEmlStream. `stream` hands the sources to
// renderEmlStream. Output is consumed chunk by chunk and only counted.
const { Readable } = require('node:stream');
const { renderEml, renderEmlStream } = require('../crates/trustmailer-node');

const mode = process.argv[2] || 'stream';
const count = Number(process.argv[3] || 10);
const mib = Number(process.argv[4] || 10);
const CHUNK = 64 * 1024;

function source(seed) {
  let sent = 0;
  const total = mib * 1024 * 1024;
  return new Readable({
    read() {
      if (sent >= total) return this.push(null);
      const chunk = Buffer.allocUnsafe(Math.min(CHUNK, total - sent));
      for (let i = 0; i < chunk.length; i++) chunk[i] = (sent + i + seed) & 0xff;
      sent += chunk.length;
      this.push(chunk);
    },
  });
}

async function readAll(readable) {
  const chunks = [];
  for await (const chunk of readable) chunks.push(chunk);
  return Buffer.concat(chunks);
}

const message = { subject: 'Draft', text: 'Draft', html: '<b>Draft</b>', headers: { 'X-Unsent': '1' } };

(async () => {
  const start = performance.now();
  let bytes = 0;
  if (mode === 'buffer') {
    const attachments = [];
    for (let i = 0; i < count; i++) {
      attachments.push({ filename: `file-${i}.bin`, content: await readAll(source(i)) });
    }
    const eml = renderEml({ ...message, attachments });
    for (let i = 0; i < eml.length; i += CHUNK) bytes += eml.subarray(i, i + CHUNK).length;
  } else if (mode === 'stream') {
    const attachments = Array.from({ length: count }, (_, i) => ({
      filename: `file-${i}.bin`,
      open: async () => source(i),
    }));
    for await (const chunk of await renderEmlStream({ ...message, attachments })) bytes += chunk.length;
  } else {
    throw new Error(`unknown mode ${mode}`);
  }
  console.log(JSON.stringify({
    mode, runtime: typeof Bun !== 'undefined' ? `bun ${Bun.version}` : `node ${process.version}`,
    platform: `${process.platform}-${process.arch}`, attachments: count, attachmentMiB: mib,
    emlMiB: +(bytes / 1024 / 1024).toFixed(1), elapsedMs: Math.round(performance.now() - start),
    peakRssMiB: +(process.resourceUsage().maxRSS / 1024).toFixed(1),
  }));
})().catch((e) => { console.error(e); process.exitCode = 1; });
