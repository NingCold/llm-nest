// Bundle and collect only. Start the native host separately through Cua launch_app.
const fs = require('node:fs'), path = require('node:path'), http = require('node:http');
const { createRequire } = require('node:module');
const root = path.resolve(__dirname, '..'), web = path.join(root, 'web');
const req = createRequire(path.join(web, 'package.json'));
const [portText, output] = process.argv.slice(2);
const port = Number(portText);
if (!Number.isInteger(port) || port < 1024 || port > 65535 || !output) throw Error('Usage: node scripts/sdk-open-benchmark.cjs PORT OUTPUT_JSONL');
const names = new Set(['sdkImport', 'sdkInit', 'grow', 'storeOpen', 'storeCipher', 'storeDatabase', 'machineLoad', 'trackedUsers', 'statusIdentity', 'statusCrossSigning', 'statusOwnDeviceTrust', 'statusOwnDeviceTrustCached']);
function stages(value) {
  return Object.fromEntries(Object.entries(value).map(([name, duration]) => {
    if (!names.has(name) || typeof duration !== 'number' || !Number.isFinite(duration) || duration < 0) throw Error('Invalid measurement');
    return [name, duration];
  }));
}
(async () => {
  if (JSON.parse(fs.readFileSync(path.join(path.dirname(req.resolve('@matrix-org/matrix-sdk-crypto-wasm')), 'package.json'), 'utf8')).version !== '18.9.0') throw Error('Expected SDK 18.9.0');
  const { build } = await import(require('node:url').pathToFileURL(req.resolve('vite')).href);
  const bundle = await build({ configFile: false, root: web, logLevel: 'warn', build: {
    write: false, target: 'es2022', rollupOptions: { input: path.join(web, 'tests/sdk-open-benchmark.ts') },
  } });
  const assets = new Map(bundle.output.map(asset => ['/' + asset.fileName, asset]));
  const entry = bundle.output.find(asset => asset.type === 'chunk' && asset.isEntry);
  http.createServer(async (request, response) => {
    try {
      const url = new URL(request.url, `http://127.0.0.1:${port}`);
      if (request.method === 'POST' && url.pathname === '/result') {
        let body = '';
        for await (const part of request) { body += part; if (body.length > 32768) throw Error('Oversized result'); }
        const value = JSON.parse(body);
        if (!['seed', 'grow', 'measure'].includes(value.mode) || typeof value.debug !== 'boolean' || typeof value.success !== 'boolean' || !Array.isArray(value.samples) || value.samples.length > 4) throw Error('Invalid result');
        const result = { sdk: '18.9.0', unit: 'ms', mode: value.mode, debug: value.debug, success: value.success, stages: stages(value.stages), samples: value.samples.map(sample => {
          if (!Number.isInteger(sample.round) || sample.round < 0 || sample.round > 4 || typeof sample.valid !== 'boolean') throw Error('Invalid sample');
          return { round: sample.round, stages: stages(sample.stages), valid: sample.valid };
        }) };
        fs.appendFileSync(path.resolve(output), JSON.stringify(result) + '\n', 'utf8');
        console.log(JSON.stringify(result)); response.end('ok'); return;
      }
      if (request.method !== 'GET') { response.writeHead(405).end(); return; }
      if (url.pathname === '/') { response.setHeader('Content-Type', 'text/html'); response.end(`<body>Running synthetic benchmark<script type="module" src="/${entry.fileName}"></script>`); return; }
      const asset = assets.get(url.pathname);
      if (!asset) { response.writeHead(404).end(); return; }
      response.setHeader('Content-Type', asset.fileName.endsWith('.wasm') ? 'application/wasm' : 'text/javascript');
      response.end(asset.type === 'chunk' ? asset.code : asset.source);
    } catch { response.writeHead(400).end('Invalid benchmark request'); }
  }).listen(port, '127.0.0.1', () => console.log(`Collector ready: http://127.0.0.1:${port}/?mode=seed (then mode=measure; same port/profile)`));
})().catch(error => { console.error(error.message); process.exitCode = 1; });
