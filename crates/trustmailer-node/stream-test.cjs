// Run against the built addon: node stream-test.cjs / bun stream-test.cjs
const assert = require('node:assert/strict');
const { spawnSync } = require('node:child_process');
const { Readable } = require('node:stream');
const { renderEml, renderEmlStream } = require('./index.js');

const tick = () => new Promise((resolve) => setTimeout(resolve, 5));

function parse(bytes) {
  const result = spawnSync('python3', ['-c', `
import sys,json,base64
from email import policy
from email.parser import BytesParser
m=BytesParser(policy=policy.default).parsebytes(sys.stdin.buffer.read())
print(json.dumps(dict(subject=str(m['Subject'] or ''), unsent=str(m['X-Unsent'] or ''), injected=m['Injected'], parts=[dict(type=p.get_content_type(),filename=p.get_filename(),data=base64.b64encode(p.get_payload(decode=True)).decode()) for p in m.walk() if not p.is_multipart()])))
`], { input: bytes, maxBuffer: 1 << 30 });
  assert.equal(result.status, 0, String(result.stderr));
  return JSON.parse(result.stdout);
}

// Date, generated Message-IDs and the multipart boundaries differ per render.
function normalize(bytes) {
  const boundaries = new Map();
  return bytes.toString('latin1')
    .replace(/^Date: [^\r\n]*\r\n/m, 'Date: <date>\r\n')
    .replace(/^Message-ID: <[0-9a-f-]{36}@localhost>\r\n/m, 'Message-ID: <generated>\r\n')
    .replace(/--trustmailer-[A-Za-z0-9]{24}/g, (b) => {
      if (!boundaries.has(b)) boundaries.set(b, `<boundary ${boundaries.size}>`);
      return boundaries.get(b);
    });
}

async function collect(stream) {
  const chunks = [];
  const reader = stream.getReader();
  while (true) {
    const { done, value } = await reader.read();
    if (done) break;
    assert.ok(value instanceof Uint8Array);
    chunks.push(value);
  }
  return Buffer.concat(chunks);
}

function chunked(bytes, size) {
  const out = [];
  for (let i = 0; i < bytes.length; i += size) out.push(bytes.subarray(i, i + size));
  return out;
}

// A ReadableStream that records opens, reads and cancels in `log`.
function tracked(log, name, chunks, { failAfter } = {}) {
  let i = 0;
  return new ReadableStream({
    pull(controller) {
      if (failAfter !== undefined && i === failAfter) {
        log.push(`${name}:fail`);
        controller.error(new Error(`${name} failed`));
        return;
      }
      if (i === chunks.length) {
        log.push(`${name}:end`);
        controller.close();
        return;
      }
      log.push(`${name}:read`);
      controller.enqueue(chunks[i++]);
    },
    cancel() { log.push(`${name}:cancel`); },
  }, { highWaterMark: 0 });
}

const base = {
  from: 'Živa <ziva@example.com>', to: '"Doe, Jane" <jane@example.com>, Bob <bob@example.com>',
  subject: 'Pozdrav 世界\r\nInjected: yes', text: 'Živjo 世界', html: '<b>Živjo 世界</b>',
  messageId: '<fixed@example.com>',
  headers: { 'X-Unsent': '1', 'X-Safe': 'value\r\nInjected: yes' },
};

const binary = Buffer.from(Array.from({ length: 256 }, (_, i) => i));
const files = [
  { filename: 'račun 世界.bin', content: Buffer.concat(Array(9).fill(binary)), contentType: 'application/octet-stream' },
  { filename: 'empty.bin', content: Buffer.alloc(0) },
  { filename: 'one.txt', content: Buffer.from('x'), contentType: 'text/plain' },
  { filename: 'exact.bin', content: binary.subarray(0, 57 * 3) },
  { filename: 'file"quote\r\nInjected: yes.bin', content: binary.subarray(0, 58) },
  { filename: 'logo.png', content: binary, cid: 'logo@example.com' },
];
const lazy = (file, open) => {
  const { content, ...rest } = file;
  return { ...rest, open };
};

(async () => {
  // Byte equality with renderEml for every source shape and awkward chunk size.
  const expected = normalize(renderEml({ ...base, attachments: files }));
  const sources = {
    'ReadableStream x1': (b) => tracked([], 's', chunked(b, 1)),
    'ReadableStream x56': (b) => tracked([], 's', chunked(b, 56)),
    'ReadableStream x57': (b) => tracked([], 's', chunked(b, 57)),
    'ReadableStream x58': (b) => tracked([], 's', chunked(b, 58)),
    'ReadableStream x114': (b) => tracked([], 's', chunked(b, 114)),
    'ReadableStream x1000': (b) => tracked([], 's', chunked(b, 1000)),
    'async generator x7': (b) => (async function* () { yield* chunked(b, 7); })(),
    'async generator with empty chunks': (b) => (async function* () {
      for (const c of chunked(b, 100)) { yield new Uint8Array(0); yield c; }
    })(),
    'Node Readable x33': (b) => Readable.from(chunked(b, 33)),
    'Uint8Array': (b) => new Uint8Array(b),
    'sync return': null,
  };
  for (const [name, make] of Object.entries(sources)) {
    const attachments = files.map((f) => lazy(f, make ? async () => make(f.content) : () => f.content));
    const actual = normalize(await collect(await renderEmlStream({ ...base, attachments })));
    assert.equal(actual, expected, name);
  }

  // Mixing in-memory and lazy attachments, the blank-envelope draft, and no attachments.
  const asLazy = (f) => lazy(f, async () => f.content);
  for (const [streamOptions, renderOptions] of [
    [{ ...base, attachments: files.map((f, i) => (i % 2 ? f : asLazy(f))) }, { ...base, attachments: files }],
    [{ from: '', to: '', subject: '', attachments: [asLazy(files[0])] }, { from: '', to: '', subject: '', attachments: [files[0]] }],
    [{ attachments: [asLazy(files[1])] }, { attachments: [files[1]] }],
    [{ ...base }, { ...base }],
    [{ ...base, attachments: [] }, { ...base, attachments: [] }],
  ]) {
    const actual = normalize(await collect(await renderEmlStream(streamOptions)));
    assert.equal(actual, normalize(renderEml(renderOptions)));
  }

  // A large attachment with chunk sizes that do not align with base64 lines.
  const large = Buffer.alloc(3 * 1024 * 1024 + 11);
  for (let i = 0; i < large.length; i++) large[i] = (i * 7919) & 0xff;
  for (const size of [65536, 57 * 1000, 1024 * 1024 + 1]) {
    const file = { filename: 'large.bin', content: large };
    const actual = normalize(await collect(await renderEmlStream({ ...base, attachments: [lazy(file, async () => Readable.from(chunked(large, size)))] })));
    assert.equal(actual, normalize(renderEml({ ...base, attachments: [file] })), `large x${size}`);
  }

  // Every base64 line is at most 76 characters and every line ends in CRLF.
  const streamed = await collect(await renderEmlStream({ ...base, attachments: files.map((f) => lazy(f, async () => tracked([], 'x', chunked(f.content, 13)))) }));
  assert.ok(!/[^\r]\n/.test(streamed.toString('latin1')), 'bare LF');
  assert.ok(!/\r(?!\n)/.test(streamed.toString('latin1')), 'bare CR');

  // An independent parser sees the same message, with no injected headers.
  const parsed = parse(streamed);
  assert.deepEqual(parsed, parse(renderEml({ ...base, attachments: files })));
  assert.equal(parsed.subject, base.subject); // RFC 2047 encoded, so not a header break.
  assert.equal(parsed.unsent, '1');
  assert.equal(parsed.injected, null);
  const attachmentsParsed = parsed.parts.filter((p) => p.filename);
  assert.deepEqual(attachmentsParsed.map((p) => p.filename).sort(), files.map((f) => f.filename).sort());
  for (const file of files) {
    const part = attachmentsParsed.find((p) => p.filename === file.filename);
    assert.equal(part.data, file.content.toString('base64'), file.filename);
  }

  // open() resolving null drops the whole part; the rest matches renderEml without it.
  {
    const keep = [files[0], files[2], files[5]];
    const actual = normalize(await collect(await renderEmlStream({
      ...base,
      attachments: files.map((f) => lazy(f, async () => (keep.includes(f) ? f.content : null))),
    })));
    assert.equal(actual, normalize(renderEml({ ...base, attachments: keep })));
    const none = await collect(await renderEmlStream({ ...base, attachments: files.map((f) => lazy(f, async () => null)) }));
    const parsedNone = parse(none);
    assert.equal(parsedNone.parts.filter((p) => p.filename).length, 0);
    assert.deepEqual(parsedNone.parts.map((p) => p.type), ['text/plain', 'text/html']);
  }

  // Lazy, ordered opening: the first source opens before the stream is returned,
  // later ones only one ahead of the source being read.
  {
    const log = [];
    const attachments = ['a', 'b', 'c'].map((name) => ({
      filename: `${name}.bin`,
      open: async () => { log.push(`${name}:open`); return tracked(log, name, chunked(binary, 100)); },
    }));
    const stream = await renderEmlStream({ text: 'body', attachments });
    await tick();
    assert.deepEqual(log, ['a:open'], 'only the first source opens before reading');
    const reader = stream.getReader();
    while (!log.includes('a:read')) await reader.read();
    await tick();
    assert.deepEqual(log, ['a:open', 'b:open', 'a:read'], 'b opens ahead of a; c waits; a is pulled lazily');
    while (!log.includes('b:read')) await reader.read();
    assert.ok(log.indexOf('c:open') > log.indexOf('a:end'), 'c opens after a ends');
    while (!(await reader.read()).done);
    assert.deepEqual(log.filter((l) => l.endsWith(':open')), ['a:open', 'b:open', 'c:open']);
    assert.ok(!log.some((l) => l.endsWith(':cancel')));
  }

  // Inline (cid) parts render first, so they open first.
  {
    const order = [];
    const attachments = [
      { filename: 'r.bin', open: async () => { order.push('regular'); return binary; } },
      { filename: 'i.png', cid: 'i@x', open: async () => { order.push('inline'); return binary; } },
    ];
    await collect(await renderEmlStream({ html: '<img src="cid:i@x">', attachments }));
    assert.deepEqual(order, ['inline', 'regular']);
  }

  // Backpressure: an unread stream does not drain its sources.
  {
    const log = [];
    const stream = await renderEmlStream({ text: 'body', attachments: [{ filename: 'a.bin', open: async () => tracked(log, 'a', chunked(large, 4096)) }] });
    await tick();
    const reader = stream.getReader();
    for (let i = 0; i < 3; i++) await reader.read();
    await tick();
    assert.ok(log.filter((l) => l === 'a:read').length <= 2, `read ${log.length} chunks for 3 output reads`);
    await reader.cancel();
    assert.equal(log.at(-1), 'a:cancel');
  }

  // A failing first open rejects before any stream exists, and nothing else opens.
  {
    const log = [];
    await assert.rejects(renderEmlStream({ attachments: [
      { filename: 'a.bin', open: async () => { log.push('a:open'); throw new Error('S3 404'); } },
      { filename: 'b.bin', open: async () => { log.push('b:open'); return binary; } },
    ] }), /S3 404/);
    assert.deepEqual(log, ['a:open']);
    await assert.rejects(renderEmlStream({ attachments: [{ filename: 'a.bin', open: () => { throw new Error('sync throw'); } }] }), /sync throw/);
    await assert.rejects(renderEmlStream({ attachments: [{ filename: 'a.bin', open: async () => 'not bytes' }] }), TypeError);
    await assert.rejects(renderEmlStream({ attachments: [{ filename: 'a.bin', content: binary, open: async () => binary }] }), /both content and open/);
    await assert.rejects(renderEmlStream({ headers: { 'Bad:Name': 'x' }, attachments: [{ filename: 'a.bin', open: async () => { throw new Error('must not open'); } }] }), /invalid custom header name/);
  }

  // A later open failure errors the stream once its part is reached.
  {
    const log = [];
    const stream = await renderEmlStream({ attachments: [
      { filename: 'a.bin', open: async () => tracked(log, 'a', chunked(binary, 100)) },
      { filename: 'b.bin', open: async () => { throw new Error('b missing'); } },
    ] });
    await assert.rejects(collect(stream), /b missing/);
    assert.ok(log.includes('a:end'));
  }

  // A mid-stream source error errors the output and cancels the source opened ahead.
  {
    const log = [];
    const stream = await renderEmlStream({ attachments: [
      { filename: 'a.bin', open: async () => tracked(log, 'a', chunked(binary, 10), { failAfter: 3 }) },
      { filename: 'b.bin', open: async () => tracked(log, 'b', chunked(binary, 10)) },
      { filename: 'c.bin', open: async () => { log.push('c:open'); return binary; } },
    ] });
    await assert.rejects(collect(stream), /a failed/);
    assert.ok(log.includes('b:cancel'), log.join());
    assert.ok(!log.includes('c:open'));
  }
  // Same for a source that yields a non-byte chunk.
  {
    const log = [];
    const stream = await renderEmlStream({ attachments: [
      { filename: 'a.bin', open: async () => (async function* () { yield binary; yield 'text'; })() },
      { filename: 'b.bin', open: async () => tracked(log, 'b', [binary]) },
    ] });
    await assert.rejects(collect(stream), /Uint8Array chunks/);
    assert.deepEqual(log, ['b:cancel']);
  }

  // Cancelling the output cancels the source being read and the one opened ahead.
  {
    const log = [];
    const stream = await renderEmlStream({ attachments: [
      { filename: 'a.bin', open: async () => tracked(log, 'a', chunked(large, 1000)) },
      { filename: 'b.bin', open: async () => tracked(log, 'b', [binary]) },
      { filename: 'c.bin', open: async () => { log.push('c:open'); return binary; } },
    ] });
    const reader = stream.getReader();
    for (let i = 0; i < 5; i++) await reader.read();
    await reader.cancel(new Error('client went away'));
    assert.ok(log.includes('a:cancel') && log.includes('b:cancel'), log.join());
    assert.ok(!log.includes('b:read') && !log.includes('c:open'), log.join());
  }
  // Cancelling while a read is waiting on a slow source does not hang.
  {
    const log = [];
    let pending;
    const slow = new ReadableStream({
      pull: () => new Promise((resolve) => { pending = resolve; }),
      cancel: () => { log.push('slow:cancel'); pending && pending(); },
    }, { highWaterMark: 0 });
    const ahead = new Readable({ read() {} });
    ahead.on('close', () => log.push('ahead:destroyed'));
    const stream = await renderEmlStream({ attachments: [
      { filename: 'slow.bin', open: async () => slow },
      { filename: 'ahead.bin', open: async () => ahead },
    ] });
    const reader = stream.getReader();
    await reader.read(); // template up to the first body
    const read = reader.read();
    await tick();
    await reader.cancel();
    assert.deepEqual(await read, { done: true, value: undefined });
    await tick();
    assert.ok(log.includes('slow:cancel') && log.includes('ahead:destroyed'), log.join());
  }

  // Cancelling while a Node.js Readable waits for data destroys it without hanging,
  // and nothing opens after cancel.
  {
    const opened = [];
    const stalled = new Readable({ read() {} });
    const stream = await renderEmlStream({ attachments: [0, 1, 2].map((i) => ({
      filename: `${i}.bin`,
      open: async () => { opened.push(i); return i === 0 ? stalled : binary; },
    })) });
    const reader = stream.getReader();
    await reader.read();
    const read = reader.read();
    await tick();
    await Promise.race([reader.cancel(), tick().then(tick).then(() => { throw new Error('cancel hung'); })]);
    assert.ok(stalled.destroyed);
    await read;
    await tick();
    assert.deepEqual(opened, [0, 1]);
  }
  // A source error errors the output at once, even while the next open() is pending.
  {
    let openedAhead;
    const stream = await renderEmlStream({ attachments: [
      { filename: 'a.bin', open: async () => tracked([], 'a', [binary], { failAfter: 1 }) },
      { filename: 'b.bin', open: () => new Promise((resolve) => { openedAhead = resolve; }) },
    ] });
    const done = assert.rejects(collect(stream), /a failed/);
    await Promise.race([done, tick().then(tick).then(() => { throw new Error('error was delayed'); })]);
    const log = [];
    openedAhead(tracked(log, 'b', [binary]));
    await tick();
    assert.deepEqual(log, ['b:cancel'], 'a source opened after the error is cancelled');
  }

  console.log('renderEmlStream equality, laziness, failure and cancellation tests passed');
})().catch((e) => { console.error(e); process.exitCode = 1; });
