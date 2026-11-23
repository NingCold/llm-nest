const { execFileSync } = require('node:child_process');
const { readFileSync, writeFileSync, mkdirSync, copyFileSync } = require('node:fs');
const path = require('node:path');
const root = path.resolve(__dirname, '..');
// sigchld 0.2.4 declares MIT but ships no license/attribution text, including
// at its published VCS revision. Disclose that gap instead of inventing an owner.
function completeRustNotices(text) {
  const sigchld = /(^={72}\r?\nMIT License\r?\nUsed by:\r?\nsigchld 0\.2\.4\r?\nSource: https:\/\/static\.crates\.io\/crates\/sigchld\/sigchld-0\.2\.4\.crate\r?\nRepository: https:\/\/github\.com\/oconnor663\/sigchld\.rs\r?\n\r?\nMIT License\r?\n\r?\n)Copyright \(c\) <year> <copyright holders>/gm;
  text = text.replace(sigchld, (_, prefix) => prefix +
    'License provenance: SPDX MIT declared by sigchld 0.2.4; the permission text below is synthesized by cargo-about.\n' +
    "Crate author metadata: Jack O'Connor (not a verified copyright-holder statement).\n" +
    'The published archive and upstream revision 07b95e2fe38b18b376b0f635f2766bf2e641b80b contain no LICENSE or copyright notice; upstream attribution remains unconfirmed.');
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
