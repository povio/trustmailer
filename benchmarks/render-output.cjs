// Compare EML output paths: bun benchmarks/render-output.cjs <base64|copy> <invoice|big> <calls>
const tm = require('../crates/trustmailer-node');
const [variant, kind, callsArg] = process.argv.slice(2);
const pdf = (kib, seed) => Buffer.from(Array.from({ length: kib * 1024 }, (_, i) => (i * seed + (i >> 7)) & 0xff));
const attachments = kind === 'invoice'
  ? [1, 2, 3].map((n) => ({ filename: `invoice_${n}.pdf`, content: pdf(200, n + 3), contentType: 'application/pdf' }))
  : [{ filename: 'big.bin', content: pdf(10 * 1024, 7), contentType: 'application/octet-stream' }];
const options = { from: 'Sender <s@example.com>', to: 'a@example.com, b@example.com', subject: 'Invoices',
  text: 'See attached.', headers: { 'X-Unsent': '1' }, attachments };
const render = {
  base64: () => Buffer.from(tm.renderEmlBase64(options), 'base64'),
  copy: () => tm.renderEml(options),
}[variant];
const calls = Number(callsArg);
const times = [];
let bytes = 0;
for (let i = 0; i < calls + 5; i++) {
  const started = performance.now();
  bytes = render().length;
  if (i >= 5) times.push(performance.now() - started);
}
times.sort((a, b) => a - b);
console.log(JSON.stringify({ variant, kind, bytes, medianMs: times[times.length >> 1], peakRssMiB: process.resourceUsage().maxRSS / 1024 }));
