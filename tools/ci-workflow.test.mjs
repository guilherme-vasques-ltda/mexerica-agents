// Contrato dos workflows: CI só em Boat/Instabox e macOS só na Namespace, permissões explícitas, actions por SHA e ci-ok sem atalho.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync, readdirSync } from 'node:fs';

const ci = readFileSync(new URL('../.github/workflows/ci.yml', import.meta.url), 'utf8');
const lines = ci.split('\n');
const jobsStart = lines.indexOf('jobs:');
const jobs = {};
let current = null;
for (const line of lines.slice(jobsStart + 1)) {
  const header = /^  ([a-z][a-z-]*):$/.exec(line);
  if (header) { current = header[1]; jobs[current] = []; continue; }
  if (current) jobs[current].push(line);
}
const body = (name) => jobs[name].join('\n');
const LABELS = ['boat-small', 'boat', 'boat-large', 'instabox-small', 'instabox-medium', 'instabox-large'];
const RUNS_ON = new Set([
  'boat-small',
  'instabox-small',
  "${{ github.run_attempt == 1 && needs.route.outputs.light || 'instabox-small' }}",
  "${{ github.run_attempt == 1 && needs.route.outputs.medium || 'instabox-medium' }}",
  "${{ github.run_attempt == 1 && needs.route.outputs.heavy || 'instabox-large' }}",
]);

test('arquivo só ASCII, só pull_request e sem gatilhos que rodam código de fora', () => {
  assert.ok(!/[^\x00-\x7F]/.test(ci), 'ci.yml deve ser ASCII');
  assert.match(ci, /^on:\n  pull_request:\n    types: \[[a-z_, ]+\]\n\n/m);
  for (const trigger of ['pull_request_target', 'workflow_run', 'issue_comment', 'schedule', 'push:', 'merge_group', 'workflow_dispatch']) {
    assert.ok(!ci.includes(trigger), `gatilho proibido: ${trigger}`);
  }
  assert.match(ci, /^permissions: \{\}$/m);
  assert.match(ci, /^concurrency:\n  group: ci-\$\{\{ github\.event\.pull_request\.number \}\}\n  cancel-in-progress: true$/m);
});

test('todo job roda em Boat ou Instabox, com permissões e timeout explícitos', () => {
  assert.ok(Object.keys(jobs).length >= 9);
  for (const [name, job] of Object.entries(jobs)) {
    const runsOn = job.filter((l) => /^    runs-on:/.test(l));
    assert.equal(runsOn.length, 1, `${name}: exatamente um runs-on`);
    assert.ok(RUNS_ON.has(runsOn[0].replace(/^    runs-on: /, '')), `${name}: runs-on fora da lista: ${runsOn[0]}`);
    assert.ok(job.some((l) => /^    permissions:/.test(l)), `${name}: permissions explícito`);
    assert.ok(job.some((l) => /^    timeout-minutes: [1-9][0-9]*$/.test(l)), `${name}: timeout-minutes`);
    assert.ok(!/container:|services:/.test(job.join('\n')), `${name}: sem container/services`);
  }
  assert.ok(!/ubuntu-|windows-|macos-|self-hosted|blacksmith|buildjet|depot-/i.test(ci), 'nenhum rótulo de runner do GitHub ou de outro fornecedor');
});

test('route só publica rótulos Boat e Instabox', () => {
  const route = body('route');
  assert.match(route, /boat\) light=boat-small; medium=boat; heavy=boat-large ;;/);
  assert.match(route, /\*\) light=instabox-small; medium=instabox-medium; heavy=instabox-large ;;/);
  assert.match(route, /^          family=instabox$/m, 'sem resposta da sonda, a rodada vai para a Instabox');
  const assigned = [...route.matchAll(/\b(?:light|medium|heavy)=([a-z-]+)/g)].map((m) => m[1]);
  assert.ok(assigned.length >= 6 && assigned.every((label) => LABELS.includes(label)));
  assert.match(route, /^    permissions:\n      actions: read$/m);
});

test('actions de terceiros fixadas por SHA completo; checkout sem credencial persistida', () => {
  const uses = [...ci.matchAll(/^\s+(?:- )?uses: (\S+)/gm)].map((m) => m[1]);
  assert.ok(uses.length > 0);
  for (const ref of uses) assert.match(ref, /^[\w.-]+\/[\w.-]+@[0-9a-f]{40}$/, `action sem SHA: ${ref}`);
  const checkouts = ci.split('uses: actions/checkout@').slice(1);
  for (const c of checkouts) assert.match(c.split('\n      - ')[0], /persist-credentials: false/);
});

test('só o job vazamento recebe secret; token de escrita não existe', () => {
  for (const [name, job] of Object.entries(jobs)) {
    const text = job.join('\n');
    if (name !== 'vazamento') assert.ok(!/\$\{\{[^}]*\bsecrets\./.test(text), `${name}: não recebe secrets`);
    assert.ok(!/: write\b/.test(text), `${name}: sem permissão de escrita`);
  }
  assert.equal((ci.match(/\$\{\{[^}]*\bsecrets\./g) || []).length, 1);
  assert.match(body('vazamento'), /VAZAMENTO_TERMOS: \$\{\{ secrets\.VAZAMENTO_TERMOS \}\}/);
});

test('ci-ok sempre roda, recusa fork e não aceita job obrigatório pulado', () => {
  const ok = body('ci-ok');
  assert.match(ok, /^    name: ci-ok$/m);
  assert.match(ok, /^    if: \$\{\{ always\(\) \}\}$/m);
  assert.match(ok, /^    needs: \[route, secrets, vazamento, semgrep, changes, ferramentas-ci, hub, desktop\]$/m);
  assert.match(ok, /^    permissions: \{\}$/m);
  assert.ok(!ok.includes('uses:'), 'ci-ok não faz checkout nem usa actions');
  assert.match(ok, /head\.repo\.full_name != github\.repository/);
  for (const job of ['route', 'secrets', 'vazamento', 'semgrep', 'changes', 'ferramentas-ci']) {
    assert.ok(ok.includes(`.${job.includes('-') ? `"${job}"` : job}.result == "success"`), `ci-ok exige ${job}`);
  }
  assert.match(ok, /if selected\("host"\) then \.hub\.result == "success" else \.hub\.result == "skipped" end/);
  assert.match(ok, /if selected\("desktop"\) then \.desktop\.result == "success" else \.desktop\.result == "skipped" end/);
  assert.match(ok, /flag\("host"\) and flag\("desktop"\) and flag\("heavy"\)/);
});

test('Hub e Desktop rodam as provas completas', () => {
  const hub = body('hub');
  assert.match(hub, /MEXERICA_EXIGIR_BWRAP: '1'/);
  assert.match(hub, /MEXERICA_MOTOR_TESTS_REQUIRED: '1'/);
  assert.match(hub, /cargo nextest run --workspace --locked --test-threads num-cpus --no-fail-fast/);
  assert.match(hub, /cargo clippy --workspace --all-targets --locked -- -D warnings/);
  assert.match(hub, /cargo fmt --all --check/);
  assert.match(hub, /motor_mexerica_ci\.py/);
  const desktop = body('desktop');
  for (const cmd of ['npm ci --no-audit --no-fund', 'npm run typecheck', 'npm test', 'npm run test:open --if-present']) assert.ok(desktop.includes(cmd), cmd);
  assert.match(desktop, /MEXERICA_CI_BASE_SHA: \$\{\{ github\.event\.pull_request\.base\.sha \}\}/);
  assert.match(desktop, /fetch-depth: 0/);
});

test('secret só no passo cujo executor veio da main', () => {
  const steps = body('vazamento').split('\n      - ').slice(1);
  const withSecret = steps.filter((st) => st.includes('secrets.VAZAMENTO_TERMOS'));
  assert.equal(withSecret.length, 1);
  assert.ok(withSecret[0].includes("if: ${{ steps.fonte.outputs.inicial == 'false' }}"));
  const publicOnly = steps.filter((st) => st.includes("steps.fonte.outputs.inicial != 'false'"));
  assert.equal(publicOnly.length, 1);
  assert.ok(!publicOnly[0].includes('secrets.'));
});

test('changes reprova se o diff falhar e cobre configuração do Cargo', () => {
  const changes = body('changes');
  assert.match(changes, /git diff --name-only --no-renames -z "\$\{parents\[0\]\}" HEAD > "\$RUNNER_TEMP\/alterados\.z"/);
  assert.match(changes, /done < "\$RUNNER_TEMP\/alterados\.z"/);
  assert.ok(!changes.includes('done < <('), 'sem substituição de processo, que engole o erro');
  for (const p of ['Cargo.toml', 'Cargo.lock', '.cargo/*', 'rustfmt.toml', 'clippy.toml']) assert.ok(changes.includes(p), p);
});

test('Semgrep usa manifesto e regras da main, nunca os do PR', () => {
  const sem = body('semgrep');
  assert.ok(sem.includes('git show "$source:.github/security/semgrep.yml"'));
  assert.ok(sem.includes('git show "$source:.github/security/semgrep-requirements.txt"'));
  assert.ok(sem.includes('-r "$RUNNER_TEMP/semgrep-politica/requirements.txt"'));
  assert.ok(sem.includes('--config "$RUNNER_TEMP/semgrep-politica/semgrep.yml"'));
  assert.ok(!/-r \.github\/|--config \.github\//.test(sem), 'nada lido direto do checkout do PR');
});

// macOS: workflow à parte, só em máquina macOS da Namespace, fora do ci-ok.
const mac = readFileSync(new URL('../.github/workflows/macos.yml', import.meta.url), 'utf8');
const macLines = mac.split('\n');
const macJobs = {};
let macCurrent = null;
for (const line of macLines.slice(macLines.indexOf('jobs:') + 1)) {
  const header = /^  ([a-z][a-z-]*):$/.exec(line);
  if (header) { macCurrent = header[1]; macJobs[macCurrent] = []; continue; }
  if (macCurrent) macJobs[macCurrent].push(line);
}
const NSCLOUD = ['nscloud-macos-tahoe-arm64-6x14', 'nscloud-macos-sequoia-arm64-6x14'];
const ACTIONS = new Set([
  'actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1',
  'actions/setup-node@949feb2413d6458794dcd2491c4babbbce0c15c1',
  'Swatinem/rust-cache@d1527c8f67ef9af357290475d4af4b0fab8f12e0',
  'actions/upload-artifact@cf430e030ddbb5b0abf93d22962f4752f3646cd9',
]);

test('só ci.yml e macos.yml existem como workflows', () => {
  assert.deepEqual(readdirSync(new URL('../.github/workflows/', import.meta.url)).sort(), ['ci.yml', 'macos.yml']);
});

test('macOS: ASCII, só pull_request com filtro de caminhos e disparo manual sem entradas', () => {
  assert.ok(!/[^\x00-\x7F]/.test(mac), 'macos.yml deve ser ASCII');
  assert.match(mac, /^on:\n  pull_request:\n    types: \[[a-z_, ]+\]\n    paths:\n(?:      - '[^'\n]+'\n)+  workflow_dispatch:\n\n/m);
  for (const trigger of ['pull_request_target', 'workflow_run', 'issue_comment', 'schedule', 'push:', 'merge_group', 'inputs:', 'workflow_call']) {
    assert.ok(!mac.includes(trigger), `gatilho proibido: ${trigger}`);
  }
  assert.match(mac, /^permissions: \{\}$/m);
  assert.match(mac, /^concurrency:\n  group: macos-\$\{\{ github\.event\.pull_request\.number \|\| github\.ref \}\}\n  cancel-in-progress: true$/m);
});

test('macOS: um job, só Namespace macOS, leitura, timeout, sem secrets e sem fork', () => {
  assert.deepEqual(Object.keys(macJobs), ['macos']);
  const job = macJobs.macos.join('\n');
  const runsOn = macJobs.macos.filter((l) => /^    runs-on:/.test(l));
  assert.equal(runsOn.length, 1);
  assert.ok(NSCLOUD.includes(runsOn[0].replace(/^    runs-on: /, '')), `runs-on fora da lista: ${runsOn[0]}`);
  assert.ok(!/ubuntu-|windows-|self-hosted|boat|instabox|-with-cache|nscloud-cache/i.test(job), 'sem runner do GitHub nem volume de cache compartilhado');
  assert.match(job, /^    permissions:\n      contents: read\n    [a-z]/m);
  const timeout = /^    timeout-minutes: (\d+)$/m.exec(job);
  assert.ok(timeout && Number(timeout[1]) <= 90, 'timeout de no máximo 90 minutos');
  assert.ok(!/\bsecrets\./.test(mac), 'macOS não recebe secrets');
  assert.ok(!/: write\b/.test(mac), 'sem permissão de escrita');
  assert.ok(!/container:|services:/.test(job));
  assert.match(job, /^    if: \$\{\{ github\.event_name == 'workflow_dispatch' \|\| \(github\.event\.pull_request\.head\.repo\.full_name == github\.repository && !github\.event\.pull_request\.draft\) \}\}$/m);
  assert.ok(!body('ci-ok').includes('macos'), 'o job macOS fica fora do ci-ok');
});

test('macOS: actions da lista fechada por SHA; checkout sem credencial persistida', () => {
  const uses = [...mac.matchAll(/^\s+(?:- )?uses: (\S+)/gm)].map((m) => m[1]);
  assert.ok(uses.length > 0);
  for (const ref of uses) assert.ok(ACTIONS.has(ref), `action fora da lista: ${ref}`);
  for (const c of mac.split('uses: actions/checkout@').slice(1)) assert.match(c.split('\n      - ')[0], /persist-credentials: false/);
});

test('macOS: roda as provas nativas, empacota e confere assinatura e arquitetura', () => {
  const job = macJobs.macos.join('\n');
  assert.match(job, /cargo test --locked --bin donnabot-host fork::open_codex/);
  assert.match(job, /CODYNC_ENV: open\n(?:.*\n)*?        run: cargo test --locked --test open_hub/);
  for (const cmd of ['npm ci --no-audit --no-fund', 'npm run test:open', 'npm run dist:mac:open', 'codesign --verify --deep --strict "$OPEN_APP"', 'test "$(lipo -archs "$hub")" = arm64']) assert.ok(job.includes(cmd), cmd);
  assert.match(job, /shasum -a 256 --check/, 'rustup-init conferido por hash');
});

test('macOS: zip e artefato só no disparo manual, com retenção curta', () => {
  const steps = macJobs.macos.join('\n').split('\n      - ').slice(1);
  const manual = "if: ${{ github.event_name == 'workflow_dispatch' }}";
  const upload = steps.filter((st) => st.includes('actions/upload-artifact@'));
  assert.equal(upload.length, 1);
  assert.ok(upload[0].includes(manual));
  const days = /retention-days: (\d+)/.exec(upload[0]);
  assert.ok(days && Number(days[1]) >= 1 && Number(days[1]) <= 7);
  assert.match(upload[0], /if-no-files-found: error/);
  const zip = steps.filter((st) => st.includes('ditto -c -k'));
  assert.equal(zip.length, 1);
  assert.ok(zip[0].includes(manual) && zip[0].includes('SHA256SUMS'));
  assert.ok(steps.some((st) => st.includes("if: ${{ always() }}") && st.includes('GITHUB_STEP_SUMMARY') && st.includes('Minutos')), 'resumo de minutos sempre');
});

