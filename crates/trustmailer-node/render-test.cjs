// Run against the built addon: node render-test.cjs / bun render-test.cjs
const assert = require('node:assert/strict');
const { spawnSync } = require('node:child_process');
const { renderEml, renderEmlBase64, createTransport } = require('./index.js');
function parse(base64) {
  const result = spawnSync('python3', ['-c', `
import sys,json,base64
from email import policy
from email.parser import BytesParser
m=BytesParser(policy=policy.default).parsebytes(base64.b64decode(sys.stdin.read()))
print(json.dumps(dict(subject=str(m['Subject'] or ''), sender=str(m['From'] or ''), to=[a.addr_spec for a in m['To'].addresses] if m['To'] else [], unsent=str(m['X-Unsent'] or ''), injected=m['Injected'], parts=[dict(type=p.get_content_type(),filename=p.get_filename(),data=base64.b64encode(p.get_payload(decode=True)).decode()) for p in m.walk() if not p.is_multipart()])))
`], { input: base64, encoding: 'utf8' });
  assert.equal(result.status, 0, result.stderr);
  return JSON.parse(result.stdout);
}
(async () => {
  for (const options of [{}, { from: '', to: '', cc: '', bcc: '' }]) {
    const draft = parse(renderEmlBase64(options));
    assert.equal(draft.sender, ''); assert.deepEqual(draft.to, []);
  }
  for (const options of [{from:'',to:'r@example.com'}, {from:'s@example.com',to:''}]) {
    parse(renderEmlBase64(options));
  }
  const injected = parse(renderEmlBase64({subject:'Report\r\nInjected: yes',
    attachments:[{filename:'file\r\nInjected: yes.bin',content:Buffer.from([0,255])}]}));
  assert.equal(injected.injected, null);
  const binary = Buffer.from(Array.from({length: 256}, (_, i) => i));
  const options = {
    from: 'Živa <ziva@example.com>', to: '"Doe, Jane" <jane@example.com>, Bob <bob@example.com>',
    subject: 'Pozdrav 世界', text: 'Živjo 世界', html: '<b>Živjo 世界</b>',
    headers: { 'X-Unsent': '1', 'X-Safe': 'value\r\nInjected: yes' },
    attachments: [
      { filename: 'račun 世界.bin', content: binary },
      { filename: 'empty.bin', content: Buffer.alloc(0) },
      { filename: 'second.bin', content: binary.toString('base64') },
    ],
  };
  const draft = parse(renderEmlBase64(options));
  assert.equal(draft.subject, options.subject);
  const bytes = renderEml(options);
  assert.ok(Buffer.isBuffer(bytes));
  const rendered = parse(bytes.toString('base64'));
  assert.equal(rendered.subject, options.subject); assert.equal(rendered.unsent, '1');
  assert.equal(rendered.parts[2].data, binary.toString('base64')); assert.match(draft.sender, /Živa/);
  assert.deepEqual(draft.to, ['jane@example.com', 'bob@example.com']);
  assert.equal(draft.unsent, '1'); assert.equal(draft.injected, null);
  assert.equal(draft.parts[0].type, 'text/plain'); assert.equal(draft.parts[1].type, 'text/html');
  assert.equal(Buffer.from(draft.parts[0].data, 'base64').toString().trimEnd(), options.text);
  assert.equal(Buffer.from(draft.parts[1].data, 'base64').toString().trimEnd(), options.html);
  assert.equal(draft.parts[2].filename, options.attachments[0].filename);
  assert.equal(draft.parts[2].data, binary.toString('base64'));
  assert.equal(draft.parts[3].data, ''); assert.equal(draft.parts[4].data, binary.toString('base64'));
  // Text attachments are files: bare LF and non-UTF-8 bytes must survive unchanged.
  const textFile = Buffer.concat([Buffer.from('line one\nline two \r\n'), Buffer.from([0x9a, 0x0a])]);
  const withText = parse(renderEml({ text: 'body', attachments: [
    { filename: 'notes.txt', content: textFile, contentType: 'text/plain' },
    { filename: 'rows.csv', content: textFile },
  ] }).toString('base64'));
  assert.equal(withText.parts[1].data, textFile.toString('base64'));
  assert.equal(withText.parts[2].data, textFile.toString('base64'));
  for (const name of ['Bad:Name', 'Bad\r\nInjected', '']) {
    assert.throws(() => renderEmlBase64({headers:{[name]: 'x'}}), /invalid custom header name/);
  }
  const stream = await createTransport({streamTransport:true}).sendMail(options);
  assert.equal(stream.response, '250 stream transport: message rendered, not sent');
  assert.equal(parse(stream.rawBase64).subject, options.subject);
  // These fail before any network or process I/O.
  for (const transport of [{host:'127.0.0.1',port:1}, {sendmail:true,path:'/nonexistent'}]) {
    const t = createTransport(transport);
    await assert.rejects(t.sendMail({}), /\[NoSender\]/);
    await assert.rejects(t.sendMail({from:'a@example.com'}), /\[NoRecipients\]/);
  }
  console.log('MIME parser, draft, stream and delivery validation regressions passed');
})().catch(e => { console.error(e); process.exitCode = 1; });
