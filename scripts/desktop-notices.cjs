const { execFileSync } = require('node:child_process');
const { readFileSync, writeFileSync, mkdirSync, copyFileSync } = require('node:fs');
const path = require('node:path');
const root = path.resolve(__dirname, '..');
function completeRustNotices(text) {
  // Apache's standard appendix is a sample, not a missing attribution.
  if (/^Copyright[^\r\n]*(?:<copyright holders>|<owner>|<year>|\[fullname\])[^\r\n]*\r?$/mi.test(text)) {
    throw Error('Unresolved placeholder copyright in desktop Rust notices; review about.toml clarifications');
  }
  return text;
}
exports.completeRustNotices = completeRustNotices;
function copyrightNotices(text) {
  const sources = JSON.parse(readFileSync(path.join(__dirname, 'licenses/copyrights.json'), 'utf8'));
  const entries = new Set([...text.matchAll(/^(\S+) (\S+)\r?\nSource: [^\r\n]*\r?\nRepository:/gm)].map(m => `${m[1]} ${m[2]}`));
  const groups = new Map();
  for (const [name, item] of Object.entries(sources)) {
    if ([...entries].some(entry => entry.startsWith(name + ' ') && entry !== `${name} ${item.version}`)) throw Error(`Review COPYRIGHT for upgraded ${name}`);
    if (!entries.has(`${name} ${item.version}`)) continue;
    if (!groups.has(item.file)) groups.set(item.file, []);
    groups.get(item.file).push(`Component attribution: ${name} ${item.version}\nSource: ${item.source}`);
  }
  return [...groups].map(([file, members]) => `${members.join('\n')}\n\n${readFileSync(path.join(__dirname, 'licenses', file), 'utf8')}`).join('\n\n');
}
exports.copyrightNotices = copyrightNotices;
function generate(target, tool, webNotices, output) {
  if (execFileSync(tool, ['--version'], { encoding: 'utf8' }).trim() !== 'cargo-about 0.9.3') throw Error('Use the verified cargo-about 0.9.3 binary');
  const rust = path.join(root, 'target', `notices-rust-${target}.txt`);
  mkdirSync(path.dirname(rust), { recursive: true });
  execFileSync(tool, ['generate', '--manifest-path', path.join(root, 'desktop/src-tauri/Cargo.toml'), '--config', path.join(root, 'about.toml'), '--target', target, '--locked', '--fail', '--output-file', rust, path.join(root, 'about.hbs')], { cwd: root, stdio: 'inherit' });
  const rustText = readFileSync(rust, 'utf8');
  const texts = [readFileSync(webNotices, 'utf8'), completeRustNotices(rustText), copyrightNotices(rustText), 'Tauri Windows installer templates\n' + readFileSync(path.join(root, 'desktop/src-tauri/packaging/windows/LICENSE.tauri'), 'utf8')];
  mkdirSync(path.dirname(output), { recursive: true });
  writeFileSync(output, texts.join('\n\n'));
  for (const name of ['LICENSE', 'NOTICE']) copyFileSync(path.join(root, name), path.join(path.dirname(output), name + '.txt'));
}
exports.generate = generate;
if (require.main === module) {
  const target = process.env.TAURI_ENV_TARGET_TRIPLE || process.env.LLMN_AGENT_TARGET;
  if (!target) throw Error('Set LLMN_AGENT_TARGET to the desktop target triple');
  generate(target, process.env.CARGO_ABOUT || 'cargo-about', path.join(root, 'desktop/dist/THIRD_PARTY_NOTICES.txt'), path.join(root, 'desktop/src-tauri/resources/THIRD_PARTY_NOTICES.txt'));
}
