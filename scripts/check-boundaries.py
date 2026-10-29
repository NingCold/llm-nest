"""Check the client workspace and fixed connector/artifact boundary."""
from pathlib import Path
import tomllib
ROOT = Path(__file__).resolve().parents[1]
workspace = tomllib.loads((ROOT / 'Cargo.toml').read_text(encoding='utf8'))['workspace']
assert workspace['members'] == ['desktop/src-tauri']
connector = workspace['dependencies']['connector']
assert connector['git'] == 'https://github.com/NingCold/llm-nest-server.git'
assert len(connector['rev']) == 40 and all(c in '0123456789abcdef' for c in connector['rev'])
prepare = (ROOT / 'desktop/scripts/prepare-agent.mjs').read_text(encoding='utf8')
assert not any(token in prepare for token in ('execFile', 'spawn(', 'cargo build', 'target/debug', 'target/release'))
print('Client workspace and Agent artifact boundaries OK')
