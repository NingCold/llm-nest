const { execFileSync } = require('node:child_process');
const { readFileSync, writeFileSync, readdirSync, mkdirSync } = require('node:fs');
const path = require('node:path');
const { createHash } = require('node:crypto');
const root = path.resolve(__dirname, '..');
function generate(licenses, overrides, overrideDirectory) {
  const sections = [];
  for (const packages of Object.values(licenses)) for (const pkg of packages) {
    const header = `${pkg.name} ${pkg.versions.join(', ')}\nDeclared license: ${pkg.license}\n${pkg.homepage || ''}`;
    const texts = [];
    for (const [index, directory] of pkg.paths.entries()) {
      const version = pkg.versions[index];
      const files = readdirSync(directory, { withFileTypes: true }).filter(f => f.isFile() && /^(licen[cs]e|copying|notice)(?:[._-].*)?$/i.test(f.name)).map(f => f.name).sort();
      texts.push(...files.map(file => `${version} ${file}\n${readFileSync(path.join(directory, file), 'utf8')}`));
      const key = `${pkg.name}@${version}`;
      const override = overrides[key];
      const needsSupplement = Object.entries(overrides).some(([name, source]) => name.startsWith(pkg.name + '@') && source.additional);
      if (!files.length || needsSupplement) {
        if (!override) throw Error(`Missing license text for ${key}; add a version-pinned source`);
        const bytes = readFileSync(path.join(overrideDirectory, override.file));
        if (createHash('sha256').update(bytes).digest('hex') !== override.sha256) throw Error(`License source SHA256 mismatch for ${key}`);
        texts.push(`Source: ${override.source}\n${override.sourceNote || ''}\n${bytes.toString('utf8')}`);
      }
    }
    sections.push({ key: header, text: `${header}\n\n${[...new Set(texts)].join('\n\n')}` });
  }
  return 'LLM Nest Web third-party notices\nIncludes installed production dependencies; some packages may be bundled only in optional paths.\n\n' + sections.sort((a,b) => a.key.localeCompare(b.key, 'en')).map(s => s.text).join('\n\n' + '='.repeat(72) + '\n\n') + '\n';
}
exports.generate = generate;
if (require.main === module) {
  const output = process.argv[2];
  if (!output) throw Error('Usage: node scripts/web-notices.cjs OUTPUT');
  const args = ['pnpm@11.22.0', '--dir', 'web', 'licenses', 'list', '--prod', '--json'];
  const text = process.platform === 'win32'
    ? execFileSync('cmd.exe', ['/d', '/s', '/c', 'corepack ' + args.join(' ')], { cwd: root, encoding: 'utf8' })
    : execFileSync('corepack', args, { cwd: root, encoding: 'utf8' });
  const overridesDir = path.join(root, 'packaging/licenses');
  const result = generate(JSON.parse(text), JSON.parse(readFileSync(path.join(overridesDir, 'sources.json'))), overridesDir);
  mkdirSync(path.dirname(path.resolve(output)), { recursive: true });
  writeFileSync(output, result);
  for (const name of ['LICENSE', 'NOTICE']) writeFileSync(path.join(path.dirname(path.resolve(output)), name + '.txt'), readFileSync(path.join(root, name)));
}
