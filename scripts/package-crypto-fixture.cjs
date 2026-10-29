const fs = require('node:fs');
const path = require('node:path');
const { createHash } = require('node:crypto');
const root = path.resolve(__dirname, '..');
const ts = require(path.join(root, 'web/node_modules/typescript'));
const output = process.argv[2];
if (!output) throw Error('Usage: node scripts/package-crypto-fixture.cjs OUTPUT_DIRECTORY');
const destination = path.resolve(output);
if (fs.existsSync(destination) && fs.readdirSync(destination).length) throw Error('Output must be empty');
fs.mkdirSync(destination, { recursive: true });
const sourceRoot = path.join(root, 'web/src');
const outputName = source => path.relative(sourceRoot, source).replaceAll(path.sep, '/').replace(/^cloud\//, '').replace(/\.tsx?$/, '.cjs');
const files = {};
function compile(source) {
  const relative = path.relative(sourceRoot, source).replaceAll(path.sep, '/').replace(/^cloud\//, '');
  if (relative === 'api.ts') return; // Integration harness injects its authenticated transport.
  const name = outputName(source);
  if (Object.hasOwn(files, name)) return;
  files[name] = '';
  let code = ts.transpileModule(fs.readFileSync(source, 'utf8'), { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 } }).outputText;
  code = code.replace(/require\(["'](\.[^"']+)["']\)/g, (_, reference) => {
    const target = path.resolve(path.dirname(source), reference);
    const dependency = fs.existsSync(target + '.ts') ? target + '.ts' : target + '.tsx';
    compile(dependency);
    let emitted = path.posix.relative(path.posix.dirname(name), outputName(dependency));
    if (!emitted.startsWith('.')) emitted = './' + emitted;
    return `require(${JSON.stringify(emitted)})`;
  });
  if (relative === 'crypto/engine.ts') code += '\nexports.seed = state => { active = { sdk: state.sdk, machine: state.machine, account: state.account, owner: ownerOf(state.account), deviceId: state.device, cursor: state.cursor }; };\n';
  const target = path.join(destination, name);
  fs.mkdirSync(path.dirname(target), { recursive: true });
  fs.writeFileSync(target, code);
  files[name] = createHash('sha256').update(code).digest('hex');
}
for (const name of ['engine', 'migration', 'content', 'attachments']) compile(path.join(sourceRoot, 'cloud/crypto', name + '.ts'));
fs.writeFileSync(path.join(destination, 'manifest.json'), JSON.stringify({ schemaVersion: 1, clientVersion: JSON.parse(fs.readFileSync(path.join(root, 'web/package.json'))).version, files }, null, 2) + '\n');
console.log(destination);
