// Contrato do workflow de CI: só runners Boat/Instabox, permissões explícitas, actions por SHA e ci-ok sem atalho.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

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
