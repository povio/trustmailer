// Sequential real MIME + base64 renders. No explicit GC; no retained outputs.
// node benchmarks/render-memory.cjs 300 10 (or bun ...)
const { renderEmlBase64 } = require('../crates/trustmailer-node');
const calls = Number(process.argv[2] || 300);
const mib = Number(process.argv[3] || 10);
const options = { headers: {'X-Unsent':'1'}, text:'Draft', html:'<b>Draft</b>',
  attachments:[{filename:'payload.bin',content:Buffer.alloc(mib*1024*1024, 0xa5)}] };
const samples = [];
const start = performance.now();
let length;
for (let i=1;i<=calls;i++) {
  length = renderEmlBase64(options).length;
  if (i % 25 === 0 || i === calls) samples.push({calls:i,rssMiB:process.memoryUsage().rss/1024/1024});
}
console.log(JSON.stringify({platform:process.platform,arch:process.arch,runtime:process.versions,
  calls,attachmentMiB:mib,base64Length:length,elapsedMs:performance.now()-start,
  peakRssMiB:process.resourceUsage().maxRSS/1024,samples},null,2));
