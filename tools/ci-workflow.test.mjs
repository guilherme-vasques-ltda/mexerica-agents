// Contrato dos workflows: CI e macOS sempre nos runners do GitHub (ubuntu-24.04 e macos-15), sem variável de máquina e sem fornecedor de terceiros, permissões explícitas, actions por SHA e ci-ok sem atalho. O release é disparo manual nos runners grátis do GitHub e só cria rascunho.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync, readdirSync, mkdtempSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { fileURLToPath } from 'node:url';
import { compilarRegras, verificar } from './ci-bastidor.mjs';
import { notify } from './ci-vazamento.mjs';
import { validarVersao, filtrarChangelog, montarNotas, gerarNotasGithub, main } from './ci-release-notes.mjs';
import { versaoSemver, aplicarVersaoPacote, conferirVersaoAsar, main as mainVersao } from './ci-release-version.mjs';

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
const GITHUB_LINUX = 'ubuntu-24.04';
const FORNECEDORES = /self-hosted|blacksmith|buildjet|depot-|boat|instabox|nscloud/i;

test('Open Code Review separa secrets e escrita, sem checkout do head ou gate de merge', () => {
  const review = readFileSync(new URL('../.github/workflows/code-review.yml', import.meta.url), 'utf8');
  const [config, analizar, publicar] = review.split(/^jobs:\n  review:\n|^  publish:\n/m);
  assert.match(config, /^on:\n  pull_request:\n    branches: \[main\]\n    types: \[opened, synchronize, reopened\]\n\npermissions: \{\}$/m);
  assert.match(config, /group: code-review-\$\{\{ github.event.pull_request.number \}\}\n  cancel-in-progress: true/);
  for (const job of [analizar, publicar]) {
    assert.match(job, /head\.repo\.fork == false && github\.event\.pull_request\.head\.repo\.full_name == github\.repository && github\.event\.pull_request\.draft == false/);
    assert.match(job, /runs-on: ubuntu-24\.04\n    timeout-minutes: [1-9]/);
    assert.match(job, /uses: actions\/checkout@[0-9a-f]{40} .*\n(?:[^\n]+\n)*?          ref: \$\{\{ github\.event\.pull_request\.base\.sha \}\}\n          persist-credentials: false/);
    assert.doesNotMatch(job, /contents: write|issues: write|alibaba\/open-code-review@|ci-ok/);
  }
  assert.doesNotMatch(analizar, /: write/);
  assert.match(publicar, /pull-requests: write/);
  assert.doesNotMatch(publicar, /secrets\.|OCR_LLM_/);
  assert.doesNotMatch(analizar, /vars\.OCR_LLM_MODEL|sysctl\s+-w/);
  assert.equal([...analizar.matchAll(/OCR_LLM_MODEL: \$\{\{ secrets\.OCR_LLM_MODEL \}\}/g)].length, 3);
  assert.match(analizar, /OCR_LLM_PROTOCOL: \$\{\{ steps\.config\.outputs\.protocol \}\}/);
  assert.match(analizar, /profile ocr-bwrap \/usr\/bin\/bwrap flags=\(unconfined\) \{\n            userns,/);
  assert.match(analizar, /apparmor_parser -R "\$profile"/);
  assert.match(analizar, /tools\/ci-code-review\.py validate-provider/);
  assert.match(analizar, /uses: actions\/upload-artifact@[a-f0-9]{40}[\s\S]*?path: \$\{\{ runner.temp \}\}\/mexerica-ocr\/review\.json\n          if-no-files-found: error\n          retention-days: 1/);
  assert.doesNotMatch(analizar, /actions: read/);
  assert.match(publicar, /permissions:\n      actions: read\n      contents: read\n      pull-requests: write\n/);
  assert.match(publicar, /gh run download "\$GITHUB_RUN_ID" --repo "\$GITHUB_REPOSITORY" \\\n\s+--name "ocr-projection-\$GITHUB_RUN_ATTEMPT" --dir "\$destination"/);
  assert.doesNotMatch(publicar, /uses: actions\/download-artifact@/);
  assert.doesNotMatch(publicar, /github-token:|run-id:|repository:|REVIEW:/);
  assert.match(analizar, /--ro-bind "\$work\/repo" \/review --ro-bind "\$work\/ocr" \/ocr --chdir \/review/);
  assert.match(analizar, /OCR_MAX_TOKENS_BUDGET > 2000000/);
  assert.match(analizar, /sha256sum --check --status/);
  assert.match(analizar, /> "\$work\/result\.json" 2> "\$work\/stderr\.log"/);
  const fixtures = spawnSync('python3', ['-B', fileURLToPath(new URL('./ci-code-review.test.py', import.meta.url))], { encoding: 'utf8' });
  assert.equal(fixtures.status, 0, fixtures.stderr || String(fixtures.error));
});

test('arquivo só ASCII, só pull_request e sem gatilhos que rodam código de fora', () => {
  assert.ok(!/[^\x00-\x7F]/.test(ci), 'ci.yml deve ser ASCII');
  assert.match(ci, /^on:\n  pull_request:\n    types: \[[a-z_, ]+\]\n\n/m);
  for (const trigger of ['pull_request_target', 'workflow_run', 'issue_comment', 'schedule', 'push:', 'merge_group', 'workflow_dispatch']) {
    assert.ok(!ci.includes(trigger), `gatilho proibido: ${trigger}`);
  }
  assert.match(ci, /^permissions: \{\}$/m);
  assert.match(ci, /^concurrency:\n  group: ci-\$\{\{ github\.event\.pull_request\.number \}\}\n  cancel-in-progress: true$/m);
});

test('todo job roda em runner fixo do GitHub, com permissões e timeout explícitos', () => {
  assert.deepEqual(Object.keys(jobs), ['leves', 'vazamento', 'hub', 'desktop', 'ci-ok']);
  for (const [name, job] of Object.entries(jobs)) {
    const runsOn = job.filter((l) => /^    runs-on:/.test(l));
    assert.equal(runsOn.length, 1, `${name}: exatamente um runs-on`);
    assert.equal(runsOn[0].replace(/^    runs-on: /, ''), GITHUB_LINUX, `${name}: runs-on fora do contrato: ${runsOn[0]}`);
    assert.ok(job.some((l) => /^    permissions:/.test(l)), `${name}: permissions explícito`);
    assert.ok(job.some((l) => /^    timeout-minutes: [1-9][0-9]*$/.test(l)), `${name}: timeout-minutes`);
    assert.ok(!/container:|services:/.test(job.join('\n')), `${name}: sem container/services`);
  }
  assert.ok(!FORNECEDORES.test(ci), 'nenhum rótulo de runner de terceiro ou self-hosted');
  assert.ok(!/\bvars\./.test(ci), 'nenhuma variável de máquina');
  assert.ok(!/Recusar maquina do GitHub/.test(ci), 'nenhuma guarda contra runner do GitHub');
  assert.ok(!/run_attempt|needs\.route/.test(ci), 'sem rota por tentativa');
});

test('leves: verificadores da main antes de qualquer código do PR; auditoria só como aviso', () => {
  const steps = body('leves').split('\n      - ').slice(1);
  const at = (needle) => steps.findIndex((st) => st.includes(needle));
  const trusted = ['id: selection', 'trusted-ci-gitleaks.mjs', 'semgrep-politica/semgrep.yml'].map(at);
  const pr = ['node --test tools/ci-gitleaks.test.mjs tools/ci-vazamento.test.mjs tools/ci-workflow.test.mjs', 'node tools/ci-security-audit.mjs'].map(at);
  assert.ok([...trusted, ...pr].every((i) => i >= 0), 'todas as checagens presentes');
  assert.ok(Math.max(...trusted) < Math.min(...pr), 'código do PR só depois dos verificadores da main');
  for (const st of steps.filter((s) => s.includes('Auditoria -'))) {
    assert.match(st, /continue-on-error: true/);
    assert.match(st, /timeout-minutes: [1-5]\n/, 'auditoria com tempo curto para não derrubar o job');
  }
  assert.equal(steps.filter((s) => s.includes('continue-on-error')).length, 2, 'só a auditoria é aviso');
  assert.ok(!/continue-on-error/.test(body('leves').split('\n    steps:')[0]), 'o job leves não é aviso');
});

test('Hub e Desktop só ligam máquina quando selecionados e fora de rascunho ou PR empilhado', () => {
  assert.match(body('hub'), /^    needs: leves\n    if: \$\{\{ needs\.leves\.outputs\.host == 'true' && needs\.leves\.outputs\.heavy == 'true' \}\}$/m);
  assert.match(body('desktop'), /^    needs: leves\n    if: \$\{\{ needs\.leves\.outputs\.desktop == 'true' && needs\.leves\.outputs\.heavy == 'true' \}\}$/m);
  const leves = body('leves');
  assert.match(leves, /\[ "\$BASE_REF" = main \] \|\| stacked=true/);
  assert.match(leves, /if \[ "\$DRAFT" = true \] \|\| \[ "\$stacked" = true \]; then heavy=false; fi/);
  assert.match(leves, /BASE_REF: \$\{\{ github\.event\.pull_request\.base\.ref \}\}/);
  for (const job of ['leves', 'vazamento']) assert.match(body(job), /^    if: \$\{\{ github\.event\.pull_request\.head\.repo\.full_name == github\.repository \}\}$/m);
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

test('ci-ok sempre roda, recusa fork, PR empilhado e job obrigatório pulado', () => {
  const ok = body('ci-ok');
  assert.match(ok, /^    name: ci-ok$/m);
  assert.match(ok, /^    if: \$\{\{ always\(\) \}\}$/m);
  assert.match(ok, /^    needs: \[leves, vazamento, hub, desktop\]$/m);
  assert.match(ok, /^    permissions: \{\}$/m);
  assert.ok(!ok.includes('uses:'), 'ci-ok não faz checkout nem usa actions');
  assert.match(ok, /head\.repo\.full_name != github\.repository/);
  assert.match(ok, /\(\.leves\.result == "success"\) and \(\.vazamento\.result == "success"\)/);
  assert.match(ok, /if \[ "\$\(jq -r '\.leves\.outputs\.stacked' <<< "\$NEEDS"\)" = true \]; then[\s\S]*?exit 1\n          fi/);
  assert.match(ok, /\(\.leves\.outputs\.stacked == "false"\)/);
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

test('seleção reprova se o diff falhar e cobre configuração do Cargo', () => {
  const changes = body('leves');
  assert.match(changes, /git diff --name-only --no-renames -z "\$\{parents\[0\]\}" HEAD > "\$RUNNER_TEMP\/alterados\.z"/);
  assert.match(changes, /done < "\$RUNNER_TEMP\/alterados\.z"/);
  assert.ok(!changes.includes('done < <('), 'sem substituição de processo, que engole o erro');
  for (const p of ['Cargo.toml', 'Cargo.lock', '.cargo/*', 'rustfmt.toml', 'clippy.toml']) assert.ok(changes.includes(p), p);
});

test('Semgrep usa manifesto e regras da main, nunca os do PR', () => {
  const sem = body('leves');
  assert.ok(sem.includes('git show "$source:.github/security/semgrep.yml"'));
  assert.ok(sem.includes('git show "$source:.github/security/semgrep-requirements.txt"'));
  assert.ok(sem.includes('-r "$RUNNER_TEMP/semgrep-politica/requirements.txt"'));
  assert.ok(sem.includes('--config "$RUNNER_TEMP/semgrep-politica/semgrep.yml"'));
  assert.ok(!/-r \.github\/|--config \.github\//.test(sem), 'nada lido direto do checkout do PR');
});

// macOS: workflow à parte, fora do ci-ok; sempre no runner macos-15 do GitHub.
const mac = readFileSync(new URL('../.github/workflows/macos.yml', import.meta.url), 'utf8');
const macLines = mac.split('\n');
const macJobs = {};
let macCurrent = null;
for (const line of macLines.slice(macLines.indexOf('jobs:') + 1)) {
  const header = /^  ([a-z][a-z-]*):$/.exec(line);
  if (header) { macCurrent = header[1]; macJobs[macCurrent] = []; continue; }
  if (macCurrent) macJobs[macCurrent].push(line);
}
const MAC_ELIGIBLE = "((github.event_name == 'workflow_dispatch' && github.ref == 'refs/heads/main') || (github.event.pull_request.head.repo.full_name == github.repository && !github.event.pull_request.draft))";
const MAC_IF = `\${{ ${MAC_ELIGIBLE} }}`;
// Avalia o texto exato da expressão do YAML: && e || devolvem o operando como no Actions, e == compara texto sem caixa.
const CONTEXTS = [
  ['github.event.pull_request.head.repo.full_name', 'ctx.headRepo'],
  ['github.event.pull_request.draft', 'ctx.draft'],
  ['github.event.repository.visibility', 'ctx.visibility'],
  ['github.event_name', 'ctx.event'],
  ['github.repository', 'ctx.repo'],
  ['github.ref', 'ctx.ref'],
  ['runner.environment', 'ctx.environment'],
];
const evalExpr = (expr, ctx) => {
  let js = expr.replace(/^\$\{\{ | \}\}$/g, '');
  for (const [name, key] of CONTEXTS) js = js.replace(new RegExp(`${name.replace(/\./g, '\\.')}\\b`, 'g'), key);
  js = js.replace(/(ctx\.\w+) (==|!=) (ctx\.\w+|'[^']*')/g, (_, x, op, y) => `(${op === '!=' ? '!' : ''}eq(${x}, ${y}))`);
  assert.ok(!/[^\s\w.'()&|!,/-]/.test(js) && !/\b(?:github|vars|runner)\./.test(js), `expressão fora do subconjunto avaliado: ${js}`);
  const eq = (x, y) => String(x ?? '').toLowerCase() === String(y ?? '').toLowerCase();
  return new Function('ctx', 'eq', `return ${js};`)(ctx, eq);
};
const VISIBILITIES = ['public', 'private', 'internal', '', undefined];
const ACTIONS = new Set([
  'actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1',
  'actions/setup-node@949feb2413d6458794dcd2491c4babbbce0c15c1',
  'Swatinem/rust-cache@d1527c8f67ef9af357290475d4af4b0fab8f12e0',
  'actions/upload-artifact@cf430e030ddbb5b0abf93d22962f4752f3646cd9',
]);

test('somente os workflows da lista fechada existem', () => {
  assert.deepEqual(readdirSync(new URL('../.github/workflows/', import.meta.url)).sort(), ['bastidor.yml', 'ci.yml', 'code-review.yml', 'cua-driver-pin.yml', 'macos.yml', 'release.yml', 'vazamento-semanal.yml', 'windows-isolation-proof.yml']);
});

const windowsProof = readFileSync(new URL('../.github/workflows/windows-isolation-proof.yml', import.meta.url), 'utf8');
const windowsLauncher = readFileSync(new URL('./ci-workflow-windows.ps1', import.meta.url), 'utf8');

test('Windows: prova isolada para a primeira abertura da pilha interna no SHA autorizado', () => {
  assert.match(windowsProof, /^on:\n  pull_request:\n    branches: \[cy\/verifica-conteno-do-processo\]\n    types: \[opened\]\n\npermissions: \{\}$/m);
  assert.match(windowsProof, /^    if: >-\n      github\.run_attempt == '1' &&\n      github\.event\.pull_request\.head\.repo\.fork == false &&\n      github\.event\.pull_request\.head\.repo\.full_name == github\.repository &&\n      github\.event\.pull_request\.head\.ref == 'ci\/windows-isolation-proof-test-diagnostics' &&\n      github\.event\.pull_request\.base\.sha == '94a5d3d8f6bd4f3922144675ddcbf125e0557f07'\n    runs-on: windows-2025\n    timeout-minutes: [1-9][0-9]*\n    permissions:\n      contents: read\n    defaults:/m);
  assert.deepEqual([...windowsProof.matchAll(/^\s*- uses: (\S+)/gm)].map((m) => m[1]), [[...ACTIONS][0]]);
  assert.match(windowsProof, /ref: \$\{\{ github\.event\.pull_request\.head\.sha \}\}\n          fetch-depth: 2\n          persist-credentials: false/);
  assert.match(windowsProof, /test "\$\(git rev-list --parents -n 1 HEAD\)" = "\$\(git rev-parse HEAD\) \$BASE_SHA"\n          git diff --exit-code "\$BASE_SHA" HEAD -- \. ':\(exclude\)\.github\/workflows\/windows-isolation-proof\.yml' ':\(exclude\)tools\/ci-workflow\.test\.mjs' ':\(exclude\)tools\/ci-workflow-windows\.ps1'$/m);
  assert.doesNotMatch(windowsProof, /secrets\.|: write|continue-on-error|cache:|artifact|environment:|ci-ok/);
});

test('Windows: bootstrap isolado e teste focal offline sob usuário padrão', () => {
  const [worker, setup] = windowsLauncher.split("if (-not $IsWindows -or $env:RUNNER_ENVIRONMENT -cne 'github-hosted'");
  assert.match(worker, /if \(\$principal\.Identity\.User\.Value -cne \$expectedSid\.Trim\(\) -or \$principal\.IsInRole\(\$admins\)\)/);
  assert.match(worker, /\$env:CARGO_NET_OFFLINE = 'true'[\s\S]*?\$module = 'fork::motor_mexerica::windows_laboratorio::'\s+\$testArgs = @\('--locked', '--target', 'x86_64-pc-windows-msvc', '--manifest-path', 'host\/Cargo\.toml', '-p', 'codync-host', '--bin', 'donnabot-host', \$module, '--', '--test-threads=1'\)/);
  assert.match(worker, /cargo test --no-run --message-format=json @testArgs > \$buildLog 2> \$null[\s\S]*?exit 20[\s\S]*?cargo test @testArgs --list 2> \$null[\s\S]*?exit 22[\s\S]*?'catalog_child', 'simulacao_nao_readmite_com_termino_indeterminado', 'windows_nativo_catalogo_e_recusa_antes_de_retomar', 'windows_nativo_identidade_da_imagem_recusa_copia_ou_hash_errado'[\s\S]*?\$listed -join "`n"\) -cne \(\$expected -join "`n"\)\) \{ exit 22 \}[\s\S]*?cargo test @testArgs > \(Join-Path \$work 'test.log'\) 2> \$null[\s\S]*?exit 21/);
  assert.match(setup, /SetAccessRuleProtection\(\$true, \$false\)/);
  assert.match(setup, /cargo\.exe'\) fetch --locked --target x86_64-pc-windows-msvc/);
  assert.match(setup, /New-LocalUser -Name \$name -Password \$password[\s\S]*?Add-LocalGroupMember -Group \$users -Member \$user[\s\S]*?Get-LocalGroupMember -SID \$admins/);
  assert.match(setup, /ReadAndExecute[\s\S]*?Modify[\s\S]*?Start-Process .* -Credential \$credential -UseNewEnvironment -LoadUserProfile .* -PassThru -Wait/);
  assert.match(setup, /switch \(\$child\.ExitCode\)[\s\S]*?20 \{[\s\S]*?\$result = 20[\s\S]*?21 \{[\s\S]*?\$result = 21[\s\S]*?finally[\s\S]*?Remove-CimInstance[\s\S]*?Remove-LocalUser -SID \$user\.SID[\s\S]*?Remove-Item -LiteralPath \$root -Recurse -Force[\s\S]*?exit \$result/);
});

test('Windows: a prova exige três casos e recusa skip, zero, duplicata e erro do executor', () => {
  const step = windowsProof.split('      - name: Run and verify the Windows isolation tests\n')[1];
  const script = step.split('        run: |\n')[1].replace(/^ {10}/gm, '');
  const prefix = 'test fork::motor_mexerica::windows_laboratorio::';
  const catalog = `${prefix}windows_nativo_catalogo_e_recusa_antes_de_retomar ... ok`;
  const identity = `${prefix}windows_nativo_identidade_da_imagem_recusa_copia_ou_hash_errado ... ok`;
  const child = `${prefix}catalog_child ... ignored, worker privado`;
  const simulation = `${prefix}simulacao_nao_readmite_com_termino_indeterminado ... ok`;
  const summary = 'test result: ok. 3 passed; 0 failed; 1 ignored; 0 measured; 42 filtered out; finished in 0.50s';
  const good = [catalog, identity, child, simulation, summary];
  const run = (lines, code = 0) => spawnSync('bash', ['-c', `pwsh() { cat; printf '%s\n' '::warning::PRIVATE_STDERR' >&2; return "$MOCK_EXIT"; }\n${script}`], {
    env: { ...process.env, MOCK_EXIT: String(code) },
    input: `${lines.join('\r\n')}\r\n`, encoding: 'utf8',
  });
  const unsafeOutput = ['::add-mask::PRIVATE_ENV', '::stop-commands::PRIVATE_COMMAND', '\x1b[31m C:\\outside\\PRIVATE_PATH', 'S-1-5-21-123-PRIVATE_SID', 'PRIVATE_LONG'.repeat(1024)];
  const passing = run([...good, ...unsafeOutput]);
  assert.equal(passing.status, 0, passing.stderr || passing.stdout || String(passing.error));
  assert.equal(passing.stdout, 'Discovered: 4\nVerified: 3 passed, 0 failed, 1 private child ignored by the harness.\nOK\n');
  assert.equal(passing.stderr, '');
  for (const bad of [
    ['test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 42 filtered out;'],
    [catalog, identity.replace('... ok', '... ignored'), child, simulation, summary],
    [catalog, catalog, identity, child, simulation, summary],
    [catalog, identity, simulation, summary],
    [catalog, identity, child, summary],
    [catalog, `${identity}\x1b[0m`, child, simulation, summary],
    [catalog, identity, child, simulation, summary.replace('1 ignored', '2 ignored')],
  ]) {
    const failure = run([...bad, ...unsafeOutput]);
    assert.equal(failure.status, 1);
    assert.match(failure.stdout, /^Discovered: 4\n(?:test [a-z_:]+ \.\.\. (?:ok|FAILED|ignored)\n)*TEST_FAILED\n$/);
    assert.equal(failure.stderr, '');
  }
  for (const [code, marker] of [[20, 'BUILD_FAILED'], [22, 'DISCOVERY_FAILED'], [7, '::error::The proof setup or cleanup failed; no test result is confirmed.']]) {
    const failure = run([...good, ...unsafeOutput], code);
    assert.equal(failure.status, 1);
    assert.equal(failure.stdout, `${marker}\n`);
    assert.equal(failure.stderr, '');
  }
  const testFailure = run([catalog.replace('... ok', '... FAILED'), identity, child + ' PRIVATE_PANIC', simulation, 'LAB_STAGE: PRIVATE_LAB', 'CHILD_STAGE: PRIVATE_CHILD', ...unsafeOutput], 21);
  assert.equal(testFailure.status, 1);
  assert.equal(testFailure.stdout, `Discovered: 4\n${catalog.replace('... ok', '... FAILED')}\n${identity}\n${simulation}\n${prefix}catalog_child ... ignored\nTEST_FAILED\n`);
  assert.equal(testFailure.stderr, '');
  assert.doesNotMatch(run(good, 21).stdout, /\nOK\n/);
  const failedCatalog = catalog.replace('... ok', '... FAILED');
  const header = `---- ${catalog.slice(5, -7)} stdout ----`;
  const block = [header, 'LAB_STAGE=TokenLpac', 'CHILD_STAGE=2'];
  const fixture = [failedCatalog, ...block, 'LAB_STAGE=Perfil::error::PRIVATE', 'CHILD_STAGE=3\x1b[0m', 'panic PRIVATE', 'failures:', 'LAB_STAGE=Job'];
  const projected = run(fixture, 21);
  assert.equal(projected.stdout, `Discovered: 4\n${failedCatalog}\nLAB_STAGE=TokenLpac\nCHILD_STAGE=2\nTEST_FAILED\n`);
  for (const ambiguous of [[failedCatalog, ...block, ...block], [catalog, ...block], [failedCatalog, ...block.slice(1)]]) {
    assert.doesNotMatch(run(ambiguous, 21).stdout, /(?:LAB|CHILD)_STAGE=/);
  }
  const build = run(['BUILD_FAILED_WORKSPACE', 'error[E0425]', '--> host/src/main.rs:12:2', '--> host/src/ignored.rs:9:1', 'error: ::warning::PRIVATE_TEXT', '--> C:\\outside\\PRIVATE_PATH:7:1', ...unsafeOutput], 20);
  assert.equal(build.status, 1);
  assert.equal(build.stdout, 'BUILD_FAILED_WORKSPACE\nerror[E0425]\n--> host/src/main.rs:12:2\n');
  assert.equal(build.stderr, '');
  assert.equal(run(['BUILD_FAILED_WORKSPACE', 'BUILD_FAILED_DEPENDENCY', 'error[E0425]'], 20).stdout, 'BUILD_FAILED\n');
  const bounded = run(['BUILD_FAILED_DEPENDENCY', ...Array(50).fill('error[E0425]'), `--> host/${'a'.repeat(301)}.rs:9:1`], 20);
  assert.equal(bounded.stdout.split('\n').filter(Boolean).length, 41);
});

test('macOS: ASCII, só pull_request com filtro de caminhos e disparo manual sem entradas', () => {
  assert.ok(!/[^\x00-\x7F]/.test(mac), 'macos.yml deve ser ASCII');
  assert.match(mac, /^on:\n  pull_request:\n    types: \[[a-z_, ]+\]\n    paths:\n(?:      - '[^'\n]+'\n)+  workflow_dispatch:\n\n/m);
  for (const trigger of ['pull_request_target', 'workflow_run', 'issue_comment', 'schedule', 'push:', 'merge_group', 'inputs:', 'workflow_call']) {
    assert.ok(!mac.includes(trigger), `gatilho proibido: ${trigger}`);
  }
  assert.match(mac, /^permissions: \{\}$/m);
  assert.match(mac, /^concurrency:\n  group: macos-\$\{\{ github\.event\.pull_request\.number \|\| github\.run_id \}\}\n  cancel-in-progress: true$/m);
});

test('macOS: um job de build, runner macos-15 do GitHub, leitura, timeout, sem secrets e sem fork', () => {
  assert.deepEqual(Object.keys(macJobs), ['macos']);
  const job = macJobs.macos.join('\n');
  const runsOn = macJobs.macos.filter((l) => /^    runs-on:/.test(l));
  assert.equal(runsOn.length, 1);
  assert.equal(runsOn[0].replace(/^    runs-on: /, ''), 'macos-15', `runs-on fora do contrato: ${runsOn[0]}`);
  assert.ok(!/ubuntu-|windows-|self-hosted|blacksmith|buildjet|depot-|boat|instabox|-with-cache|nscache|nscloud/i.test(job), 'nenhum rótulo de outro fornecedor ou volume de cache compartilhado');
  assert.ok(!/\bvars\./.test(mac), 'nenhuma variável de máquina');
  assert.match(job, /^    permissions:\n      contents: read\n    [a-z]/m);
  const timeout = /^    timeout-minutes: (\d+)$/m.exec(job);
  assert.ok(timeout && Number(timeout[1]) <= 90, 'timeout de no máximo 90 minutos');
  assert.ok(!/\bsecrets\./.test(mac), 'macOS não recebe secrets');
  assert.ok(!/: write\b/.test(mac), 'sem permissão de escrita');
  assert.ok(!/container:|services:/.test(job));
  assert.ok(job.includes(`\n    if: ${MAC_IF}\n`));
  assert.ok(!body('ci-ok').includes('macos'), 'o job macOS fica fora do ci-ok');
});

test('macOS: disparo manual fora da main exata para antes do checkout e do build', () => {
  const steps = macJobs.macos.join('\n').split('\n      - ').map((st) => st.replace(/^(?:      #.*\n)+/, ''));
  const refuse = steps.findIndex((st) => st.startsWith('name: Recusar disparo manual fora da main'));
  const checkout = steps.findIndex((st) => st.startsWith('uses: actions/checkout@'));
  assert.ok(refuse > 0 && refuse < checkout, 'checagem exata antes do checkout');
  assert.ok(steps[refuse].includes("if: ${{ github.event_name == 'workflow_dispatch' }}"));
  assert.match(steps[refuse], /\n        run: \|\n          test "\$GITHUB_REF" = refs\/heads\/main \|\| \{ echo '::error::[^'\n]+'; exit 1; \}(?:\n      #.*)*$/);
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
  for (const cmd of ['npm ci --no-audit --no-fund', 'npm run test:open', 'npm run dist:mac:open', 'codesign --verify --deep --strict "$OPEN_APP"', 'test "$(lipo -archs "$hub")" = arm64', 'codesign --verify --strict "$hub"', 'codesign --verify --strict "$speech"']) assert.ok(job.includes(cmd), cmd);
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
  assert.match(zip[0], /\n        run: \|\n          test "\$GITHUB_REF" = refs\/heads\/main\n/, 'zip só da main: primeiro comando do passo, sem desvio de erro');
  assert.ok(zip[0].includes('-teste-ci-${GITHUB_SHA::12}.zip'), 'nome do zip diz teste de CI e o commit');
  assert.ok(zip[0].includes('nao e versao oficial') && zip[0].includes('GITHUB_STEP_SUMMARY'), 'resumo avisa que não é versão oficial');
  assert.match(upload[0], /name: mexerica-open-macos-arm64-teste-ci-\$\{\{ github\.sha \}\}/);
  assert.ok(steps.some((st) => st.includes("if: ${{ always() }}") && st.includes('GITHUB_STEP_SUMMARY') && st.includes('Minutos')), 'resumo de minutos sempre');
});

test('seleção por caminhos: o laço do workflow marca Hub e Desktop pelos arquivos que os testes leem', () => {
  const lines = body('leves').split('\n');
  const start = lines.indexOf('          host=false');
  const end = lines.indexOf('          done < "$RUNNER_TEMP/alterados.z"');
  assert.ok(start > 0 && end > start, 'laço de seleção encontrado');
  const script = lines.slice(start, end + 1).map((l) => l.slice(10)).join('\n') + '\nprintf "%s %s" "$host" "$desktop"\n';
  const select = (paths) => {
    const dir = mkdtempSync(join(tmpdir(), 'ci-selecao-'));
    try {
      writeFileSync(join(dir, 'alterados.z'), paths.map((p) => p + '\0').join(''));
      const r = spawnSync('bash', ['-c', script], { env: { PATH: process.env.PATH, RUNNER_TEMP: dir }, encoding: 'utf8' });
      assert.equal(r.status, 0, r.stderr);
      return r.stdout;
    } finally { rmSync(dir, { recursive: true, force: true }); }
  };
  assert.equal(select(['host/src/fork/conectores/fichas/exemplo.json']), 'true true', 'fichas de conector rodam o Desktop');
  assert.equal(select(['rust-toolchain']), 'true true', 'nome legado do toolchain roda o Hub');
  assert.equal(select(['rust-toolchain.toml']), 'true true');
  assert.equal(select(['host/src/main.rs']), 'true false', 'só Hub dispensa o Desktop');
  assert.equal(select(['motor/package.json']), 'true false');
  assert.equal(select(['apps/desktop/src/main/index.ts']), 'false true');
  assert.equal(select(['docs/guides/ci.md']), 'false false', 'o guia do CI só roda leves');
  assert.equal(select(['docs/features/marketplace.md']), 'false true', 'outros documentos continuam ligando o Desktop');
  assert.equal(select(['.github/workflows/ci.yml']), 'true true', 'o próprio ci.yml muda os jobs pesados');
  assert.equal(select(['tools/ci-vazamento.mjs']), 'false false', 'ferramentas de CI rodam só em leves');
  assert.equal(select(['tools/ci-workflow.test.mjs', 'docs/guides/ci.md', '.github/workflows/macos.yml', '.github/actionlint.yaml']), 'false false', 'workflow do macOS, guia e ferramentas de CI não ligam Hub nem Desktop');
  assert.equal(select(['.github/workflows/release.yml', 'tools/ci-release-notes.mjs']), 'false false', 'workflow de release e gerador de notas rodam só em leves');
  assert.equal(select(['tools/ci-release-version.mjs']), 'false false', 'gravador de versão da release roda só em leves');
  assert.equal(select(['.github/security/vazamento.json']), 'false false');
  assert.equal(select(['tools/textos-legado.json']), 'false true', 'arquivo de ferramenta lido pelos testes do Desktop continua ligando o Desktop');
  assert.equal(select(['docs/guides/idiomas.md']), 'false true', 'doc lido pelos testes do Desktop continua ligando o Desktop');
  assert.equal(select(['docs/guides/ci.md', 'host/src/main.rs']), 'true false', 'um arquivo que liga o Hub não é anulado pelos que não ligam nada');
  assert.equal(select(['.github/workflows/macos.yml', 'apps/desktop/src/main/index.ts']), 'false true');
});

test('hub: devolve a origem de /etc/localtime ao dono root antes da suíte', () => {
  const steps = body('hub').split('\n      - ').map((st) => st.replace(/^(?:      #.*\n)+/, ''));
  const tz = steps.findIndex((st) => st.startsWith('name: Devolver a origem de /etc/localtime ao dono root'));
  const suite = steps.findIndex((st) => st.startsWith('name: Executar toda a suite do Hub (nextest)'));
  assert.ok(tz > 0 && tz < suite, 'passo de timezone antes da suíte');
  const st = steps[tz];
  assert.ok(st.includes('readlink -f /etc/localtime'));
  assert.ok(st.includes('sudo -n chown root:root'));
  assert.ok(st.includes('sudo -n chmod g-w,o-w'));
  // Sem sudo o passo falha fechado; sem violação ele não pede sudo.
  assert.match(st, /fora" = 0[\s\S]*exit 0/);
  assert.match(st, /sudo -n true \|\|/, 'sem sudo, erro claro');
  assert.match(st, /continua fora do contrato do sandbox/, 'verifica de novo no fim');
  assert.match(st, /\[ -e "\$alvo" \] \|\| \{ echo "origem de \/etc\/localtime ausente \(o sandbox ignora\): \$alvo"; exit 0; \}/, 'origem ausente e ignorada como no sandbox');
});

test('hub: garante o bubblewrap de verdade (mapa de UID) antes da suíte', () => {
  const steps = body('hub').split('\n      - ').map((st) => st.replace(/^(?:      #.*\n)+/, ''));
  const bwrap = steps.findIndex((st) => st.startsWith('name: Garantir bubblewrap'));
  const suite = steps.findIndex((st) => st.startsWith('name: Executar toda a suite do Hub (nextest)'));
  assert.ok(bwrap > 0 && bwrap < suite, 'passo antes da suíte');
  const st = steps[bwrap];
  assert.match(st, /sudo -n apparmor_parser -r \/etc\/apparmor\.d\/bwrap/);
  assert.match(st, /sudo -n sysctl -w kernel\.apparmor_restrict_unprivileged_userns=0/);
  assert.match(st, /bwrap --unshare-user --ro-bind \/ \/ true/);
  assert.match(st, /::error::bwrap nao consegue criar o mapa de UID/);
  assert.match(st, /\[ -f \/etc\/apparmor\.d\/bwrap \]/, 'perfil oficial primeiro');
  assert.match(st, /apparmor_parser -r \/etc\/apparmor\.d\/bwrap \|\| echo '::warning::/, 'carregar o perfil e tolerante');
  assert.ok(st.includes("[ \"${RUNNER_ENVIRONMENT:-}\" = 'github-hosted' ]"), 'sysctl so no runner do GitHub');
  assert.match(st, /sysctl -w kernel\.apparmor_restrict_unprivileged_userns=0 \|\| echo '::warning::/, 'sysctl tolerante');
});

test('leves e vazamento não ligam cache automático do setup-node', () => {
  for (const job of ['leves', 'vazamento']) {
    const setup = body(job).split('uses: actions/setup-node@')[1].split('\n      - ')[0];
    assert.match(setup, /package-manager-cache: false/, job);
  }
});

test('Semgrep: Python em modo isolado e fora da árvore analisada', () => {
  const leves = body('leves');
  assert.match(leves, /- name: Semgrep - validar plataforma e instalar Semgrep OSS fixado\n        working-directory: \$\{\{ runner\.temp \}\}/);
  for (const cmd of ["python3 -I -c 'import sys", 'python3 -I -m venv', 'cd "$RUNNER_TEMP"\n          python3 -I - <<']) assert.ok(leves.includes(cmd), cmd);
  assert.ok(!/python3 (?!-I)/.test(leves), 'nenhum python3 sem -I');
});

// Release: disparo manual multiplataforma; runners grátis do GitHub, só repositório público, rascunho sem publicar.
const rel = readFileSync(new URL('../.github/workflows/release.yml', import.meta.url), 'utf8');
const relLines = rel.split('\n');
const relJobs = {};
let relCurrent = null;
for (const line of relLines.slice(relLines.indexOf('jobs:') + 1)) {
  const header = /^  ([a-z][a-z-]*):$/.exec(line);
  if (header) { relCurrent = header[1]; relJobs[relCurrent] = []; continue; }
  if (relCurrent) relJobs[relCurrent].push(line);
}
const relBody = (name) => relJobs[name].join('\n');
const relSteps = (name) => relBody(name).split('\n    steps:\n')[1].split('\n      - ').map((st) => st.replace(/^(?:      #.*\n)+/, ''));
const REL_IF = "${{ github.event_name == 'workflow_dispatch' && github.ref == 'refs/heads/main' && github.event.repository.visibility == 'public' }}";
const REL_GUARD = "${{ runner.environment != 'github-hosted' || github.event.repository.visibility != 'public' }}";
const REL_VERSAO_ERRO = "'::error::O workflow de release so roda a partir da main.'";

test('release: ASCII, só workflow_dispatch com mode e version, sem gatilho proibido e sem secrets', () => {
  assert.ok(!/[^\x00-\x7F]/.test(rel), 'release.yml deve ser ASCII');
  assert.match(rel, /^on:\n  workflow_dispatch:\n    inputs:\n      mode:\n        description: '[^'\n]+'\n        type: choice\n        required: true\n        default: 'release'\n        options: \['release', 'preview'\]\n      version:\n        description: '[^'\n]+'\n        type: string\n        required: false\n        default: 'v0\.0\.1'\n\n/m);
  for (const trigger of ['pull_request', 'pull_request_target', 'workflow_call', 'workflow_run', 'issue_comment', 'schedule', 'push:', 'merge_group']) {
    assert.ok(!rel.includes(trigger), `gatilho proibido: ${trigger}`);
  }
  assert.match(rel, /^permissions: \{\}$/m);
  assert.match(rel, /^concurrency:\n  group: release-\$\{\{ github\.ref \}\}\n  cancel-in-progress: false$/m);
  assert.ok(!/secrets\./.test(rel), 'release sem secrets');
  assert.ok(!/container:|services:/.test(rel), 'sem container/services');
});

test('release: quatro jobs nos runners grátis do GitHub, com timeout e sem rótulo pago', () => {
  assert.deepEqual(Object.keys(relJobs), ['build-macos', 'build-linux', 'build-windows', 'draft-release']);
  const esperados = { 'build-macos': 'macos-15', 'build-linux': 'ubuntu-24.04', 'build-windows': 'windows-latest', 'draft-release': 'ubuntu-24.04' };
  for (const [name, job] of Object.entries(relJobs)) {
    const texto = job.join('\n');
    const runsOn = job.filter((l) => /^    runs-on:/.test(l));
    assert.equal(runsOn.length, 1, `${name}: exatamente um runs-on`);
    assert.equal(runsOn[0].replace(/^    runs-on: /, ''), esperados[name], `${name}: runs-on do contrato`);
    const timeout = /^    timeout-minutes: (\d+)$/m.exec(texto);
    assert.ok(timeout && Number(timeout[1]) > 0 && Number(timeout[1]) <= 90, `${name}: timeout de no máximo 90 minutos`);
    assert.ok(!/ubuntu-latest|self-hosted|blacksmith|buildjet|depot-|boat|instabox|nscloud/i.test(texto), `${name}: nenhum rótulo variável, pago ou de outro fornecedor`);
  }
  assert.match(relBody('build-windows'), /^    defaults:\n      run:\n        shell: bash$/m);
});

test('release: separação de privilégios; escrita só no draft-release e nenhum secret', () => {
  for (const name of ['build-macos', 'build-linux', 'build-windows']) {
    assert.match(relBody(name), /^    permissions:\n      contents: read$/m, `${name}: só contents read`);
    assert.ok(!/: write\b/.test(relBody(name)), `${name}: sem permissão de escrita`);
  }
  assert.match(relBody('draft-release'), /^    needs: \[build-macos, build-linux, build-windows\]$/m);
  assert.match(relBody('draft-release'), /^    permissions:\n      contents: write\n      actions: read$/m);
  assert.equal((rel.match(/GH_TOKEN: \$\{\{ github\.token \}\}/g) || []).length, 5, 'github.token só no draft-release');
});

test('release: actions da lista fechada por SHA; checkout sem credencial persistida', () => {
  const uses = [...rel.matchAll(/^\s+(?:- )?uses: (\S+)/gm)].map((m) => m[1]);
  assert.ok(uses.length > 0);
  for (const ref of uses) assert.ok(ACTIONS.has(ref), `action fora da lista: ${ref}`);
  for (const c of rel.split('uses: actions/checkout@').slice(1)) assert.match(c.split('\n      - ')[0], /persist-credentials: false/);
});

test('release: condição dos quatro jobs só libera disparo manual na main de repositório público', () => {
  for (const name of Object.keys(relJobs)) {
    const linha = relJobs[name].find((l) => l.startsWith('    if: '));
    assert.equal(linha, `    if: ${REL_IF}`, `${name}: condição do contrato`);
  }
  // O Actions compara texto sem caixa: refs/heads/MAIN passa na condição e a barreira do passo a para.
  for (const event of ['workflow_dispatch', 'push', 'pull_request', 'schedule', '']) {
    for (const ref of ['refs/heads/main', 'refs/heads/MAIN', 'refs/heads/outro', '']) {
      for (const visibility of VISIBILITIES) {
        const ctx = { event, ref, visibility };
        const esperado = String(event).toLowerCase() === 'workflow_dispatch' && String(ref).toLowerCase() === 'refs/heads/main' && visibility === 'public';
        assert.equal(evalExpr(REL_IF, ctx), esperado, `condição do job: ${JSON.stringify(ctx)}`);
      }
    }
  }
});

test('release: barreiras antes do checkout nos quatro jobs; inputs nunca interpolados em run', () => {
  for (const name of Object.keys(relJobs)) {
    const steps = relSteps(name);
    assert.ok(steps[0].startsWith(`      - name: Recusar repositorio privado ou maquina fora do GitHub\n        if: ${REL_GUARD}\n        working-directory: \${{ runner.temp }}\n        run: |\n          echo '::error::O build de release so roda em maquina gratis do GitHub com o repositorio publico.'\n          exit 1`), `${name}: guarda de repositório e máquina no primeiro passo`);
    assert.ok(steps[1].startsWith(`name: Validar branch main e parametros da rodada\n        working-directory: \${{ runner.temp }}\n        env:\n          RELEASE_MODE: \${{ inputs.mode }}\n          RELEASE_VERSION: \${{ inputs.version }}\n`), `${name}: parâmetros chegam por env no segundo passo`);
    for (const trecho of [
      'set -euo pipefail',
      `test "$GITHUB_REF" = refs/heads/main || { echo ${REL_VERSAO_ERRO}; exit 1; }`,
      '[[ "$GITHUB_SHA" =~ ^[0-9a-f]{40}$ ]]',
      'case "$RELEASE_MODE" in',
      "test \"${#RELEASE_VERSION}\" -le 64",
      '[[ "$RELEASE_VERSION" =~ ^v(0|[1-9][0-9]*)\\.(0|[1-9][0-9]*)\\.(0|[1-9][0-9]*)(-((0|[1-9][0-9]*)|[0-9]*[a-z][a-z0-9]*)(\\.((0|[1-9][0-9]*)|[0-9]*[a-z][a-z0-9]*))*)?$ ]]',
      '[[ "$RELEASE_VERSION" != *..* ]]',
      '[[ "$RELEASE_VERSION" != *-.* ]]',
      '[[ "$RELEASE_VERSION" != *. ]]',
      'RELEASE_LABEL="preview-${GITHUB_SHA:0:12}"',
      '>> "$GITHUB_ENV"',
    ]) assert.ok(steps[1].includes(trecho), `${name}: falta ${trecho}`);
    assert.ok(steps[2].startsWith('uses: actions/checkout@'), `${name}: checkout só depois das duas barreiras`);
  }
  // O semver e exportado apenas nos jobs de build; o draft-release nao empacota nada.
  for (const name of ['build-macos', 'build-linux', 'build-windows']) {
    for (const trecho of [
      'RELEASE_SEMVER="${RELEASE_VERSION#v}"',
      'RELEASE_SEMVER=""',
      'printf \'RELEASE_LABEL=%s\\nRELEASE_SEMVER=%s\\n\' "$RELEASE_LABEL" "$RELEASE_SEMVER" >> "$GITHUB_ENV"',
    ]) assert.ok(relSteps(name)[1].includes(trecho), `${name}: falta ${trecho}`);
  }
  assert.ok(!relBody('draft-release').includes('RELEASE_SEMVER'), 'draft-release não exporta semver');
  for (const environment of ['github-hosted', 'self-hosted', '', undefined]) {
    for (const visibility of VISIBILITIES) {
      assert.equal(evalExpr(REL_GUARD, { environment, visibility }), environment !== 'github-hosted' || visibility !== 'public', `guarda: ${environment} ${visibility}`);
    }
  }
  for (const linha of rel.split('\n')) {
    if (linha.includes('${{ inputs.')) assert.match(linha, /^\s*(?:if: |RELEASE_(?:MODE|VERSION): )/, `inputs fora de env/if: ${linha}`);
  }
});

test('release: a versão entra nos metadados do app entre o npm ci e o build, só no modo release', () => {
  const depoisDe = {
    'build-macos': 'Empacotar a Open (perfil oficial, assinatura ad hoc)',
    'build-linux': 'Preparar a configuracao e construir o app',
    'build-windows': 'Preparar a configuracao e construir o app',
  };
  for (const [name, seguinte] of Object.entries(depoisDe)) {
    const steps = relSteps(name).map((st) => st.replace(/^      - /, ''));
    const idx = (trecho) => steps.findIndex((st) => st.startsWith(`name: ${trecho}`));
    const npmCi = idx('Dependencias do app');
    const versao = idx('Aplicar versao da release aos metadados do app');
    assert.ok(npmCi >= 0 && versao === npmCi + 1 && idx(seguinte) === versao + 1, `${name}: passo da versão entre o npm ci e o build`);
    const passo = steps[versao];
    assert.ok(passo.includes("if: ${{ inputs.mode == 'release' }}"), `${name}: passo da versão só no modo release`);
    assert.ok(passo.includes('RELEASE_VERSION: ${{ inputs.version }}'), `${name}: versão chega por env, nunca em run`);
    assert.ok(passo.includes('node tools/ci-release-version.mjs --package apps/desktop/package.json --version "$RELEASE_VERSION"'), `${name}: chamada do gravador de versão`);
  }
});

test('release: macOS segue o padrão provado do macos.yml, empacota e confere assinatura', () => {
  const job = relBody('build-macos');
  assert.match(job, /^    env:\n      OPEN_APP: dist\/open\/mac-arm64\/Mexerica Open\.app$/m);
  for (const cmd of [
    'node-version: \'24.22.3\'',
    'cache: npm',
    'cache-dependency-path: apps/desktop/package-lock.json',
    'test "$(uname -s)" = Darwin',
    'test "$(uname -m)" = arm64',
    '63752a1919a3a4793173ec70f8f212621a7ce38ba6a246095984482f4de20f9f',
    'rustup show active-toolchain || rustup toolchain install',
    'npm ci --no-audit --no-fund',
    'npm run dist:mac:open',
    'codesign --verify --deep --strict "$OPEN_APP"',
    'test "$(lipo -archs "$hub")" = arm64',
    'codesign --verify --strict "$speech"',
    'ditto -c -k --sequesterRsrc --keepParent "$OPEN_APP" "$out/$zip"',
    'shasum -a 256 "$zip" > macos.sha256',
    'if [ -n "${RELEASE_SEMVER:-}" ]; then',
    "/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' \"$OPEN_APP/Contents/Info.plist\"",
    ')" = "$RELEASE_SEMVER"',
  ]) assert.ok(job.includes(cmd), cmd);
  assert.match(job, /key: release-macos/);
  const rustCache = job.split('uses: Swatinem/rust-cache@')[1].split('\n      - ')[0];
  assert.match(rustCache, /save-if: false/, 'o build de release só lê o cache do Cargo');
  const upload = job.split('uses: actions/upload-artifact@')[1].split('\n      - ')[0];
  assert.match(upload, /name: release-macos-arm64/);
  assert.match(upload, /path: \$\{\{ runner\.temp \}\}\/release-macos\//);
  assert.match(upload, /retention-days: 7/);
  assert.match(upload, /if-no-files-found: error/);
});

test('release: Linux empacota deb e AppImage, confere o conteúdo e gera hash', () => {
  const job = relBody('build-linux');
  for (const cmd of [
    'node tools/account-config.mjs main',
    'npm run build',
    'npx electron-builder --linux deb AppImage --publish never',
    'dpkg-deb --info "$out/$deb_name"',
    'dpkg-deb --contents "$out/$deb_name"',
    'opt/Mexerica/donnabot$',
    'opt/Mexerica/resources/account-config.json$',
    'avisos/LICENSE$',
    'avisos/NOTICE$',
    'usr/share/applications/donnabot.desktop$',
    'chmod +x "$out/$appimage_name"',
    'test -s "$out/$appimage_name"',
    'file "$out/$appimage_name" | grep -q \'ELF 64-bit LSB executable, x86-64\'',
    'sha256sum "$deb_name" "$appimage_name" > linux.sha256',
    'if [ -n "${RELEASE_SEMVER:-}" ]; then',
    'test "$(dpkg-deb -f "$out/$deb_name" Version)" = "${RELEASE_SEMVER//-/~}"',
  ]) assert.ok(job.includes(cmd), cmd);
  assert.ok(!job.includes('Version)" = "$RELEASE_SEMVER"'), 'a versão do .deb é conferida no formato Debian, com - virando ~');
  assert.match(job, /deb_name="mexerica-open-\$\{RELEASE_LABEL\}-linux-amd64\.deb"/);
  assert.match(job, /appimage_name="mexerica-open-\$\{RELEASE_LABEL\}-linux-x86_64\.AppImage"/);
  assert.ok(!job.includes('sha256sum "$deb" "$appimage"'), 'hash do Linux com nome relativo, não com caminho absoluto');
  const upload = job.split('uses: actions/upload-artifact@')[1].split('\n      - ')[0];
  assert.match(upload, /name: release-linux-amd64/);
  assert.match(upload, /path: \$\{\{ runner\.temp \}\}\/release-linux\//);
});

test('release: Windows compila o Hub nativo, valida o cua-driver e empacota NSIS e portátil', () => {
  const job = relBody('build-windows');
  for (const cmd of [
    'rustup target add x86_64-pc-windows-msvc',
    'cargo build --locked --release --target x86_64-pc-windows-msvc --bin donnabot-host --bin donnabot-hostw',
    'cp target/x86_64-pc-windows-msvc/release/donnabot-host.exe ../apps/desktop/build/native/',
    'cp target/x86_64-pc-windows-msvc/release/donnabot-hostw.exe ../apps/desktop/build/native/',
    'node packaging/cua-driver/fetch.mjs windows-x86_64 apps/desktop/build/native',
    './apps/desktop/build/native/donnabot-host.exe --version',
    'CSC_IDENTITY_AUTO_DISCOVERY: \'false\'',
    'npx electron-builder --win nsis --publish never',
    'dist/donnabot-windows-x64.exe',
    'dist/win-unpacked/Mexerica.exe',
    'dist/win-unpacked/resources/donnabot-host.exe',
    'dist/win-unpacked/resources/donnabot-hostw.exe',
    'dist/win-unpacked/resources/cua-driver.exe',
    'dist/win-unpacked/resources/account-config.json',
    'win_zip=$(cygpath -w "$out/$zip")',
    '/c/Windows/System32/tar.exe -a -c -f "$win_zip" .',
    '/c/Windows/System32/tar.exe -tf "$win_zip" | grep -Eq \'^(\\./)?Mexerica\\.exe$\'',
    'sed \'s/ \\*/  /\' | tr -d \'\\r\' > windows.sha256',
    'test "$(node -p "require(\'./apps/desktop/package.json\').version")" = "$RELEASE_SEMVER"',
    'node tools/ci-release-version.mjs --check-asar apps/desktop/dist/win-unpacked/resources/app.asar --version "$RELEASE_VERSION"',
  ]) assert.ok(job.includes(cmd), cmd);
  const conferencia = job.split('\n      - ').find((st) => st.includes('name: Conferir a versao gravada nos metadados do Windows'));
  assert.ok(conferencia, 'passo de conferência da versão no Windows');
  assert.ok(!conferencia.includes('working-directory:'), 'conferência roda da raiz do repo, onde o script e os caminhos existem');
  assert.match(job, /CODYNC_ENV: main/);
  assert.match(job, /key: release-windows/);
  const rustCache = job.split('uses: Swatinem/rust-cache@')[1].split('\n      - ')[0];
  assert.match(rustCache, /save-if: false/, 'o build de release só lê o cache do Cargo');
  const upload = job.split('uses: actions/upload-artifact@')[1].split('\n      - ')[0];
  assert.match(upload, /name: release-windows-x64/);
  assert.match(upload, /path: \$\{\{ runner\.temp \}\}\/release-windows\//);
});

test('release: draft-release baixa artefatos com gh, confere hashes e cria só rascunho', () => {
  const job = relBody('draft-release');
  assert.ok(job.includes('gh run download "$RUN_ID" --repo "$REPO" -n release-macos-arm64 -n release-linux-amd64 -n release-windows-x64 -D "$RUNNER_TEMP/downloads"'));
  for (const trecho of [
    'for par in release-macos-arm64/macos.sha256 release-linux-amd64/linux.sha256 release-windows-x64/windows.sha256',
    'test -f "$RUNNER_TEMP/downloads/$par"',
    'sha256sum -c "$(basename "$par")"',
    '[ ! -f "$caminho" ] || [ -L "$caminho" ] || [ ! -s "$caminho" ]',
    'sha256sum \\\n              "Mexerica-Open-${RELEASE_LABEL}-macos-arm64.zip" \\\n              "mexerica-open-${RELEASE_LABEL}-linux-amd64.deb" \\\n              "mexerica-open-${RELEASE_LABEL}-linux-x86_64.AppImage" \\\n              "mexerica-open-${RELEASE_LABEL}-windows-x64-setup.exe" \\\n              "mexerica-open-${RELEASE_LABEL}-windows-x64-portable.zip" \\\n              | LC_ALL=C sort -k2,2 > SHA256SUMS.txt',
    'sha256sum -c SHA256SUMS.txt',
    'node tools/ci-release-notes.mjs --version "$RELEASE_VERSION" --sha "$GITHUB_SHA" --out "$RUNNER_TEMP/release-notes.md"',
    'gh api --paginate "repos/$REPO/git/matching-refs/tags/$RELEASE_VERSION" | jq -s \'add // []\' > "$RUNNER_TEMP/tags.json"',
    'any(.[]; .ref == $ref)',
    'Ja existe release publicada para $RELEASE_VERSION.',
    'gh api -X DELETE "repos/$REPO/releases/$id"',
    'gh release create "$RELEASE_VERSION" --repo "$REPO" --draft --prerelease --target "$GITHUB_SHA" --title "Mexerica Open $RELEASE_VERSION" --notes-file "$RUNNER_TEMP/release-notes.md"',
    '$assets/Mexerica-Open-${RELEASE_LABEL}-macos-arm64.zip',
    '$assets/mexerica-open-${RELEASE_LABEL}-linux-x86_64.AppImage',
    '$assets/mexerica-open-${RELEASE_LABEL}-windows-x64-portable.zip',
    '$assets/SHA256SUMS.txt',
    '.draft == true and .prerelease == true and .target_commitish == $sha and (.assets | length) == 6',
    '"::error::Rascunho da versao $RELEASE_VERSION nao encontrado."',
    'GITHUB_STEP_SUMMARY',
  ]) assert.ok(job.includes(trecho), `falta ${trecho}`);
  assert.equal((job.match(/releases\?per_page=100" \| jq -s 'add \/\/ \[\]' > "\$RUNNER_TEMP\/releases\.json"/g) || []).length, 2, 'as duas listas de releases passam pelo jq -s da paginação');
  assert.ok(!job.includes('*.zip *.deb'), 'SHA256SUMS.txt sem glob: os cinco arquivos vêm pelo nome');
  const preview = job.split('\n      - ').filter((st) => st.includes("if: ${{ inputs.mode == 'preview' }}"));
  assert.ok(preview.some((st) => st.includes('name: mexerica-open-preview-${{ github.sha }}')), 'preview sobe artefato consolidado do Actions');
  assert.ok(preview.some((st) => st.includes('nao e versao oficial')), 'resumo da prévia avisa que não é versão oficial');
  assert.equal((job.match(/if: \$\{\{ inputs\.mode == 'release' \}\}/g) || []).length, 4, 'quatro passos do modo release');
});

// Gerador de notas de release: mesmas regras de bastidor, falha fechada, texto honesto.
const configBastidor = JSON.parse(readFileSync(new URL('../.github/security/bastidor.json', import.meta.url), 'utf8'));
const regrasBastidor = compilarRegras(configBastidor, null);

test('notas: validarVersao aceita o formato fechado e recusa o resto', () => {
  for (const ok of ['v0.0.1', 'v1.2.3', 'v1.2.3-rc.1', 'v0.0.1-beta.11', 'v1.2.3-0', 'v1.2.3-0a', 'v0.0.1-' + 'a'.repeat(57)]) assert.equal(validarVersao(ok), true, ok.length === 64 ? '64 caracteres passam' : ok);
  for (const ruim of ['0.0.1', 'V0.0.1', 'v0.0.1; rm -rf', 'v0.0.1-..', 'v0.0.1-a..b', 'v0.0.1-.1', 'v0.0.1-RC1', 'v0.0.1.', 'v0.0.1-', 'v0.0.1 ', 'v01.2.3', 'v1.02.3', 'v1.2.03', 'v1.2.3-01', 'v1.2.3-rc.01', 'v0.0.1-' + 'a'.repeat(58), '', null, undefined, 42]) {
    assert.equal(validarVersao(ruim), false, String(ruim));
  }
});

test('notas: a ERE estrita do workflow casa com as versões válidas e rejeita as outras', () => {
  // A ERE vem do próprio release.yml: o teste reprova se alguém descompassar os dois lados.
  const linha = rel.split('\n').find((l) => l.includes('RELEASE_VERSION" =~ ^v(0|'));
  assert.ok(linha, 'guarda do workflow com a ERE estrita de SemVer');
  const ere = linha.slice(linha.indexOf('=~ ') + 3, linha.indexOf(' ]]'));
  // A guarda do workflow e o par limite de 64 caracteres + ERE; a prova roda os dois, como no job.
  const casa = (versao) => spawnSync('bash', ['-c', `test "\${#1}" -le 64 && [[ "$1" =~ ${ere} ]]`, 'bash', versao]).status;
  for (const ok of ['v0.0.1', 'v1.2.3-rc.1', 'v1.2.3-0', 'v1.2.3-0a', 'v0.0.1-' + 'a'.repeat(57)]) assert.equal(casa(ok), 0, ok);
  for (const ruim of ['v01.2.3', 'v1.2.3-01', 'v0.0.1-' + 'a'.repeat(58)]) assert.notEqual(casa(ruim), 0, ruim);
});

test('notas: filtrarChangelog mantém o comum e omite bastidor sem imprimir a linha', () => {
  const bruto = [
    '* Correção do aviso de conexão',
    'Notas escritas por Claude',
    'conversa jam_0123456789abcdef',
    'Closes GUI-1234',
    '* Botão de exportar',
    '',
  ].join('\n');
  const { linhas, omitidas } = filtrarChangelog(bruto, regrasBastidor);
  assert.deepEqual(linhas, ['* Correção do aviso de conexão', '* Botão de exportar']);
  assert.equal(omitidas, 3);
});

test('notas: filtrarChangelog omite o título da API sem contar achado e rebaixa cabeçalhos', () => {
  const bruto = [
    "## What's Changed",
    '* Nova tela de ajustes',
    '## New Contributors',
    '* @alguem fez a primeira contribuicao',
    '',
  ].join('\n');
  const { linhas, omitidas } = filtrarChangelog(bruto, regrasBastidor);
  assert.deepEqual(linhas, ['* Nova tela de ajustes', '#### New Contributors', '* @alguem fez a primeira contribuicao']);
  assert.equal(omitidas, 0, 'o cabeçalho da API sai sem virar achado de bastidor');
});

test('notas: montarNotas é honesto, lista os seis arquivos e passa limpo pelo bastidor', () => {
  const sha = '0123456789abcdef0123456789abcdef01234567';
  const notas = montarNotas({ version: 'v0.2.7', sha, changelog: ['* Nova tela de ajustes', '* Botão de exportar'], regras: regrasBastidor });
  for (const trecho of [
    'Mexerica Open v0.2.7',
    'Mexerica-Open-v0.2.7-macos-arm64.zip',
    'mexerica-open-v0.2.7-linux-amd64.deb',
    'mexerica-open-v0.2.7-linux-x86_64.AppImage',
    'mexerica-open-v0.2.7-windows-x64-setup.exe',
    'mexerica-open-v0.2.7-windows-x64-portable.zip',
    'SHA256SUMS.txt',
    'Gatekeeper',
    'SmartScreen',
    'awk -v f="Mexerica-Open-v0.2.7-macos-arm64.zip" \'$2 == f\' SHA256SUMS.txt | shasum -a 256 -c -',
    'awk -v f="mexerica-open-v0.2.7-linux-amd64.deb" \'$2 == f\' SHA256SUMS.txt | sha256sum -c -',
    'Get-FileHash',
    'sudo apt install ./mexerica-open-v0.2.7-linux-amd64.deb',
    'Executar assim mesmo',
    `Commit: ${sha}`,
    '* Nova tela de ajustes',
  ]) assert.ok(notas.includes(trecho), trecho);
  assert.deepEqual(verificar({ titulo: 'Mexerica Open v0.2.7', descricao: notas }, regrasBastidor), [], 'o texto final não pode carregar bastidor');
  const semMudancas = montarNotas({ version: 'v0.0.1', sha, changelog: [], regras: regrasBastidor });
  assert.ok(semMudancas.includes('Primeira versão de pré-lançamento publicada a partir da branch main.'), 'changelog vazio ganha frase simples');
  assert.throws(() => montarNotas({ version: 'v0.0.1', sha, changelog: ['Feito com Claude'], regras: regrasBastidor }), (erro) => erro.controlado === true, 'linha de bastidor reprova o texto inteiro');
});

test('notas: gerarNotasGithub faz o POST certo e falha controlado sem vazar o token', async () => {
  const chamadas = [];
  const aceito = async (url, opcoes) => { chamadas.push({ url, opcoes }); return { status: 200, json: async () => ({ body: '* Nota da API' }) }; };
  const corpo = await gerarNotasGithub({ repo: 'dono/repo', version: 'v0.0.1', sha: '0'.repeat(40), token: 'token-ficticio', apiUrl: 'https://api.example.invalid' }, aceito);
  assert.equal(corpo, '* Nota da API');
  assert.equal(chamadas.length, 1);
  assert.equal(chamadas[0].url, 'https://api.example.invalid/repos/dono/repo/releases/generate-notes');
  assert.equal(chamadas[0].opcoes.method, 'POST');
  assert.deepEqual(JSON.parse(chamadas[0].opcoes.body), { tag_name: 'v0.0.1', target_commitish: '0'.repeat(40) });
  assert.equal(chamadas[0].opcoes.headers.Authorization, 'Bearer token-ficticio');
  assert.equal(chamadas[0].opcoes.headers.Accept, 'application/vnd.github+json');
  assert.equal(chamadas[0].opcoes.headers['X-GitHub-Api-Version'], '2022-11-28');
  assert.equal(chamadas[0].opcoes.headers['Content-Type'], 'application/json');
  assert.ok(chamadas[0].opcoes.signal, 'chamada com prazo curto');
  const recusas = [
    async () => ({ status: 404, json: async () => ({}) }),
    async () => ({ status: 401, json: async () => ({ message: 'bad credentials token-ficticio' }) }),
    async () => { throw new TypeError('rede caiu com token-ficticio'); },
  ];
  for (const buscar of recusas) {
    await assert.rejects(
      () => gerarNotasGithub({ repo: 'dono/repo', version: 'v0.0.1', sha: '0'.repeat(40), token: 'token-ficticio', apiUrl: 'https://api.example.invalid' }, buscar),
      (erro) => erro.controlado === true && !erro.message.includes('token-ficticio'),
      'erro controlado e sem o token na mensagem',
    );
  }
  for (const entrada of [{ repo: 'dono/repo', version: '0.0.1' }, { repo: '../ruim', version: 'v0.0.1' }, { repo: 'dono/repo', version: 'v0.0.1', sha: 'abc' }]) {
    await assert.rejects(() => gerarNotasGithub({ ...entrada, sha: entrada.sha ?? '0'.repeat(40), token: 't', apiUrl: 'https://api.example.invalid' }), (erro) => erro.controlado === true);
  }
});

test('notas: main gera o arquivo com changelog limpo e avisa as linhas omitidas', async (t) => {
  const log = t.mock.method(console, 'log', () => {});
  const dir = mkdtempSync(join(tmpdir(), 'notas-release-'));
  try {
    const saida = join(dir, 'release-notes.md');
    const buscar = async () => ({ status: 200, json: async () => ({ body: '* Nova tela de ajustes\nFeito com Claude\n' }) });
    const codigo = await main(['--version', 'v0.1.0', '--sha', '0'.repeat(40), '--out', saida], { GITHUB_REPOSITORY: 'dono/repo', GH_TOKEN: 't', GITHUB_API_URL: 'https://api.example.invalid' }, buscar);
    assert.equal(codigo, 0);
    const texto = readFileSync(saida, 'utf8');
    assert.ok(texto.includes('Mexerica Open v0.1.0'));
    assert.ok(texto.includes('* Nova tela de ajustes'));
    assert.ok(!texto.includes('Claude'), 'linha de bastidor não chega ao arquivo');
    assert.ok(log.mock.calls.some((c) => String(c.arguments[0]).includes('1 linha(s) omitida(s) do resumo automatico pela verificacao de bastidor.')), 'aviso genérico das linhas omitidas');
  } finally {
    rmSync(dir, { recursive: true, force: true });
    log.mock.restore();
  }
  await assert.rejects(() => main(['--version', '0.1.0', '--sha', '0'.repeat(40), '--out', 'x.md']), (erro) => erro.controlado === true, 'versão sem v reprova fechado');
});

test('notas: só o macOS promete perfil isolado; Linux e Windows avisam o perfil padrão', () => {
  const notas = montarNotas({ version: 'v0.2.7', sha: '0'.repeat(40), changelog: [], regras: regrasBastidor });
  assert.ok(notas.includes('Apenas o pacote de macOS usa o'), 'o aviso delimita qual pacote é isolado');
  assert.ok(notas.includes('perfil isolado `Mexerica Open`; os pacotes de Linux e Windows usam o perfil padrão do app'), 'o aviso nomeia o perfil padrão para Linux e Windows');
  assert.ok(notas.includes('### Como instalar e abrir'), 'título novo da seção de instalação');
  assert.ok(!notas.includes('Como abrir sem mexer em instalação existente'), 'o título antigo, que prometia isolamento geral, saiu');
  assert.ok(!notas.includes('use pastas separadas da instalação'), 'a frase genérica de isolamento saiu');
  const secao = notas.slice(notas.indexOf('### Arquivos incluídos'), notas.indexOf('### Como instalar e abrir'));
  const pacotes = secao.split('\n').filter((l) => l.startsWith('- `') && !l.includes('SHA256SUMS.txt'));
  for (const l of pacotes) {
    if (l.includes('-macos-arm64.zip')) assert.ok(l.includes('perfil isolado') && l.includes('separado de instalações existentes'), l);
    else assert.ok(l.includes('perfil padrão') && l.includes('não isolado'), `Linux e Windows sem promessa de isolamento: ${l}`);
  }
  assert.deepEqual(verificar({ titulo: 'Mexerica Open v0.2.7', descricao: notas }, regrasBastidor), [], 'o texto novo continua limpo de bastidor');
});

test('notas: o comando de hash do Linux confere só o arquivo baixado e falha fechado', () => {
  const notas = montarNotas({ version: 'v0.2.7', sha: '0'.repeat(40), changelog: [], regras: regrasBastidor });
  const linha = notas.split('\n').find((l) => l.includes('SHA256SUMS.txt | sha256sum -c -'));
  assert.ok(linha, 'as notas trazem o comando de hash do Linux');
  const comando = linha.trim();
  const dir = mkdtempSync(join(tmpdir(), 'hash-notas-'));
  try {
    const nome = 'mexerica-open-v0.2.7-linux-amd64.deb';
    const conteudo = 'conteudo ficticio do deb';
    const hash = createHash('sha256').update(conteudo).digest('hex');
    const rodar = () => spawnSync('bash', ['-c', comando], { cwd: dir, encoding: 'utf8' });
    // 1) arquivo presente, mesmo com outro arquivo do manifesto ausente no disco: passa para o baixado.
    writeFileSync(join(dir, nome), conteudo);
    writeFileSync(join(dir, 'SHA256SUMS.txt'), `${hash}  ${nome}\n${'a'.repeat(64)}  mexerica-open-v0.2.7-linux-x86_64.AppImage\n`);
    assert.equal(rodar().status, 0, 'passa com o arquivo baixado presente');
    // 2) arquivo ausente no disco: falha.
    rmSync(join(dir, nome));
    assert.notEqual(rodar().status, 0, 'falha com o arquivo ausente no disco');
    // 3) arquivo fora do manifesto: o awk não acha a linha e o sha256sum reprova a entrada vazia.
    writeFileSync(join(dir, nome), conteudo);
    writeFileSync(join(dir, 'SHA256SUMS.txt'), `${'a'.repeat(64)}  outro-arquivo.deb\n`);
    assert.notEqual(rodar().status, 0, 'falha com o arquivo fora do manifesto');
    // 4) arquivo alterado: hash divergente.
    writeFileSync(join(dir, 'SHA256SUMS.txt'), `${'b'.repeat(64)}  ${nome}\n`);
    assert.notEqual(rodar().status, 0, 'falha com o hash divergente');
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test('versao: versaoSemver tira o v e reprova versão inválida', () => {
  assert.equal(versaoSemver('v0.0.1'), '0.0.1');
  assert.equal(versaoSemver('v0.0.1-rc.1'), '0.0.1-rc.1');
  for (const ruim of ['0.0.1', 'v01.2.3', 'v1.2.3-01', 'v0.0.1-.1', 'v0.0.1-..', '', null, undefined]) {
    assert.throws(() => versaoSemver(ruim), (erro) => erro.controlado === true, String(ruim));
  }
});

// Cabeçalho do asar: [0-3]=4, [4-7]=bloco, [8-11]=payload, [12-15]=JSON, [16..]=JSON, arquivos em 8+bloco.
const montarAsarSintetico = (caminho, versao, semPackage) => {
  const conteudo = Buffer.from(`${JSON.stringify({ name: 'app-ficticio', version: versao }, null, 2)}\n`);
  const arquivos = semPackage ? {} : { 'package.json': { offset: '0', size: conteudo.length } };
  const json = Buffer.from(JSON.stringify({ files: arquivos }));
  const payload = Buffer.alloc(4 + json.length + ((4 - ((4 + json.length) % 4)) % 4));
  payload.writeUInt32LE(json.length, 0);
  json.copy(payload, 4);
  const bloco = 4 + payload.length;
  const arquivo = Buffer.alloc(8 + bloco + conteudo.length);
  arquivo.writeUInt32LE(4, 0);
  arquivo.writeUInt32LE(bloco, 4);
  arquivo.writeUInt32LE(payload.length, 8);
  payload.copy(arquivo, 12);
  conteudo.copy(arquivo, 8 + bloco);
  writeFileSync(caminho, arquivo);
};


test('versao: aplicarVersaoPacote grava só o campo version e confirma na releitura', () => {
  const dir = mkdtempSync(join(tmpdir(), 'versao-pkg-'));
  try {
    const caminho = join(dir, 'package.json');
    const original = { name: 'app-ficticio', version: '0.0.0', main: 'out/main/index.js', scripts: { build: 'vazio' } };
    writeFileSync(caminho, `${JSON.stringify(original, null, 2)}\n`);
    assert.equal(aplicarVersaoPacote(caminho, 'v1.2.3-rc.4'), '1.2.3-rc.4');
    const gravado = readFileSync(caminho, 'utf8');
    assert.match(gravado, /\n$/);
    const pkg = JSON.parse(gravado);
    assert.equal(pkg.version, '1.2.3-rc.4');
    const { version, ...resto } = pkg;
    const { version: fora, ...esperado } = original;
    assert.deepEqual(resto, esperado, 'só o campo version muda');
    assert.throws(() => aplicarVersaoPacote(caminho, '1.2.3'), (erro) => erro.controlado === true, 'versão sem v reprova');
    assert.throws(() => aplicarVersaoPacote(caminho, 'v01.2.3'), (erro) => erro.controlado === true, 'zero à esquerda reprova');
    assert.throws(() => aplicarVersaoPacote(caminho, 'v1.2.3-rc.01'), (erro) => erro.controlado === true, 'zero à esquerda no sufixo reprova');
    writeFileSync(join(dir, 'sem-version.json'), '{"name":"x"}');
    assert.throws(() => aplicarVersaoPacote(join(dir, 'sem-version.json'), 'v1.0.0'), (erro) => erro.controlado === true, 'package.json sem version reprova');
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test('versao: conferirVersaoAsar lê o package.json de dentro do app.asar', () => {
  const dir = mkdtempSync(join(tmpdir(), 'versao-asar-'));
  try {
    montarAsarSintetico(join(dir, 'ok.asar'), '3.2.1-rc.7', false);
    assert.equal(conferirVersaoAsar(join(dir, 'ok.asar'), 'v3.2.1-rc.7'), '3.2.1-rc.7');
    assert.throws(() => conferirVersaoAsar(join(dir, 'ok.asar'), 'v3.2.1'), (erro) => erro.controlado === true, 'versão divergente dentro do asar reprova');
    montarAsarSintetico(join(dir, 'sem-package.asar'), '3.2.1', true);
    assert.throws(() => conferirVersaoAsar(join(dir, 'sem-package.asar'), 'v3.2.1'), (erro) => erro.controlado === true, 'asar sem package.json reprova fechado');
    montarAsarSintetico(join(dir, 'truncado.asar'), '3.2.1', false);
    const inteiro = readFileSync(join(dir, 'truncado.asar'));
    writeFileSync(join(dir, 'truncado.asar'), inteiro.subarray(0, 20));
    assert.throws(() => conferirVersaoAsar(join(dir, 'truncado.asar'), 'v3.2.1'), (erro) => erro.controlado === true, 'asar truncado reprova fechado');
    // offset que apontaria antes da base dos arquivos reprova fechado
    montarAsarSintetico(join(dir, 'offset-negativo.asar'), '3.2.1', false);
    const inteiro2 = readFileSync(join(dir, 'offset-negativo.asar'));
    const tamanhoJson = inteiro2.readUInt32LE(12);
    const cabecalho = JSON.parse(inteiro2.subarray(16, 16 + tamanhoJson).toString('utf8'));
    cabecalho.files['package.json'].offset = '-8';
    const novoJson = Buffer.from(JSON.stringify(cabecalho));
    const copia = Buffer.from(inteiro2);
    copia.writeUInt32LE(novoJson.length, 12);
    novoJson.copy(copia, 16);
    writeFileSync(join(dir, 'offset-negativo.asar'), copia);
    assert.throws(() => conferirVersaoAsar(join(dir, 'offset-negativo.asar'), 'v3.2.1'), (erro) => erro.controlado === true, 'offset antes da base dos arquivos reprova fechado');
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test('versao: a CLI da versão funciona de ponta a ponta, com --package e --check-asar', () => {
  const dir = mkdtempSync(join(tmpdir(), 'versao-cli-'));
  const executavel = fileURLToPath(new URL('../tools/ci-release-version.mjs', import.meta.url));
  try {
    // main() importado: as duas chaves com hifen chegam onde devem.
    montarAsarSintetico(join(dir, 'app.asar'), '3.2.1', false);
    assert.equal(mainVersao(['--check-asar', join(dir, 'app.asar'), '--version', 'v3.2.1']), '3.2.1', '--check-asar pela função main');
    writeFileSync(join(dir, 'package.json'), '{"name":"app-ficticio","version":"0.0.0"}\n');
    assert.equal(mainVersao(['--package', join(dir, 'package.json'), '--version', 'v1.0.0-rc.2']), '1.0.0-rc.2', '--package pela função main');
    assert.equal(JSON.parse(readFileSync(join(dir, 'package.json'), 'utf8')).version, '1.0.0-rc.2');
    assert.throws(() => mainVersao(['--check-asar', join(dir, 'app.asar'), '--package', join(dir, 'package.json'), '--version', 'v1.0.0']), (erro) => erro.controlado === true, 'os dois argumentos juntos recusam');
    assert.throws(() => mainVersao(['--desconhecido', 'x']), (erro) => erro.controlado === true, 'argumento desconhecido recusado');
    // CLI de verdade, pelo node: o caminho feliz e o erro controlado com exit 1.
    const ok = spawnSync(process.execPath, [executavel, '--check-asar', join(dir, 'app.asar'), '--version', 'v3.2.1'], { encoding: 'utf8' });
    assert.equal(ok.status, 0, ok.stderr);
    const ruim = spawnSync(process.execPath, [executavel, '--check-asar', join(dir, 'app.asar'), '--version', '3.2.1'], { encoding: 'utf8' });
    assert.equal(ruim.status, 1, 'versão inválida encerra com exit 1');
    assert.match(ruim.stdout, /::error title=Versao da release::/, 'erro sai como anotação controlada');
    const gravar = spawnSync(process.execPath, [executavel, '--package', join(dir, 'package.json'), '--version', 'v2.0.0'], { encoding: 'utf8' });
    assert.equal(gravar.status, 0, gravar.stderr);
    assert.equal(JSON.parse(readFileSync(join(dir, 'package.json'), 'utf8')).version, '2.0.0');
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

// Workflows agendados: conferencia do pin do cua-driver e varredura semanal de vazamento.
// Rodam so em runner fixo do GitHub, sem gatilho que traga codigo de fora; o semanal guarda
// o executor e a politica da main e so notifica em destino privado confirmado com credenciais proprias.
const cuaPin = readFileSync(new URL('../.github/workflows/cua-driver-pin.yml', import.meta.url), 'utf8');
const semanal = readFileSync(new URL('../.github/workflows/vazamento-semanal.yml', import.meta.url), 'utf8');
const AGENDADOS = [['cua-driver-pin.yml', cuaPin], ['vazamento-semanal.yml', semanal]];
const jobsDe = (texto) => {
  const linhas = texto.split('\n');
  const saida = {};
  let atual = null;
  for (const linha of linhas.slice(linhas.indexOf('jobs:') + 1)) {
    const cabecalho = /^  ([a-z][a-z-]*):$/.exec(linha);
    if (cabecalho) { atual = cabecalho[1]; saida[atual] = []; continue; }
    if (atual) saida[atual].push(linha);
  }
  return saida;
};

test('agendados: ASCII, so schedule e disparo manual, runner fixo do GitHub, permissoes sem escrita', () => {
  for (const [nome, texto] of AGENDADOS) {
    assert.ok(!/[^\x00-\x7F]/.test(texto), `${nome} deve ser ASCII`);
    assert.match(texto, /^on:\n  schedule:\n    - cron: '[0-9*,/ -]+'\n  workflow_dispatch:\n\npermissions:\n  contents: read\n\njobs:\n/m, `${nome}: gatilhos e permissao de topo fora do contrato`);
    for (const acionador of ['pull_request', 'pull_request_target', 'workflow_run', 'issue_comment', 'push:', 'merge_group', 'workflow_call', 'inputs:']) {
      assert.ok(!texto.includes(acionador), `${nome}: gatilho proibido: ${acionador}`);
    }
    assert.ok(!/self-hosted|blacksmith|buildjet|depot-|boat|instabox|nscloud/i.test(texto), `${nome}: nenhum runner de terceiro ou self-hosted`);
    assert.ok(!/\bvars\.CI_MAQUINA|\$\{\{ runner\.environment/.test(texto), `${nome}: sem variavel de maquina`);
    const jobs = jobsDe(texto);
    assert.ok(Object.keys(jobs).length > 0, `${nome}: sem jobs`);
    for (const [job, linhas] of Object.entries(jobs)) {
      const runsOn = linhas.filter((l) => /^    runs-on:/.test(l));
      assert.equal(runsOn.length, 1, `${nome}.${job}: exatamente um runs-on`);
      assert.equal(runsOn[0], '    runs-on: ubuntu-24.04', `${nome}.${job}: runs-on fora do contrato`);
      assert.ok(linhas.some((l) => /^    permissions:/.test(l)), `${nome}.${job}: permissões explícitas`);
      assert.ok(linhas.some((l) => /^    timeout-minutes: [1-9][0-9]*$/.test(l)), `${nome}.${job}: timeout-minutes`);
      assert.ok(!/: write\b/.test(linhas.join('\n')), `${nome}.${job}: sem permissão de escrita`);
      assert.ok(!/container:|services:/.test(linhas.join('\n')), `${nome}.${job}: sem container/services`);
    }
    const uses = [...texto.matchAll(/^\s+(?:- )?uses: (\S+)/gm)].map((m) => m[1]);
    assert.ok(uses.length > 0, `${nome}: sem actions`);
    for (const ref of uses) {
      assert.match(ref, /^[\w.-]+\/[\w.-]+@[0-9a-f]{40}$/, `${nome}: action sem SHA completo: ${ref}`);
      assert.ok(ACTIONS.has(ref), `${nome}: action fora da lista fechada aprovada: ${ref}`);
    }
    for (const trecho of texto.split('uses: actions/checkout@').slice(1)) {
      assert.match(trecho.split('\n      - ')[0], /persist-credentials: false/, `${nome}: checkout com credencial persistida`);
    }
  }
});

test('cua-driver-pin: leitura pura, sem segredos e compara so o driver.json', () => {
  assert.ok(!/\$\{\{[^}]*\bsecrets\./.test(cuaPin), 'o pin do cua-driver nao recebe segredos');
  assert.match(cuaPin, /sparse-checkout: packaging\/cua-driver\/driver\.json/);
  assert.match(cuaPin, /GH_TOKEN: \$\{\{ github\.token \}\}/);
  assert.ok(!/VAZAMENTO|ISSUE_REPO/.test(cuaPin), 'sem credenciais de notificacao no pin');
});

test('vazamento-semanal: executor da main, credenciais nomeadas e aviso so quando a varredura reprova', () => {
  const jobs = jobsDe(semanal);
  assert.deepEqual(Object.keys(jobs), ['varredura', 'notificar']);
  const varredura = jobs.varredura.join('\n');
  assert.match(varredura, /^    if: github\.ref == 'refs\/heads\/main'$/m, 'varredura so na main');
  assert.match(varredura, /git show "\$trusted_main:tools\/ci-vazamento\.mjs" > "\$RUNNER_TEMP\/trusted-ci-vazamento\.mjs"/, 'executor vem da main');
  assert.equal((semanal.match(/VAZAMENTO_TERMOS: \$\{\{ secrets\.VAZAMENTO_TERMOS \}\}/g) || []).length, 2, 'secret de termos declarado na checagem e na varredura');
  assert.match(semanal, /551f6fc83ea457d62a0d98237cbad105af8d557003051f41f3e7ca7b3f2470eb/, 'Gitleaks fixado por SHA256');
  const credenciais = varredura.split('\n      - ').find((st) => st.startsWith('name: Recusar credenciais de aviso ausentes'));
  assert.ok(credenciais, 'checagem fail-fast das credenciais de aviso');
  assert.match(credenciais, /VAZAMENTO_ISSUE_REPO: \$\{\{ vars\.VAZAMENTO_ISSUE_REPO \}\}/);
  assert.match(credenciais, /VAZAMENTO_ISSUE_TOKEN: \$\{\{ secrets\.VAZAMENTO_ISSUE_TOKEN \}\}/);
  assert.match(credenciais, /VAZAMENTO_TERMOS: \$\{\{ secrets\.VAZAMENTO_TERMOS \}\}/, 'termos conferidos antes de qualquer download');
  assert.match(credenciais, /test -n "\$VAZAMENTO_ISSUE_REPO"/);
  assert.match(credenciais, /test -n "\$VAZAMENTO_ISSUE_TOKEN"/);
  assert.match(credenciais, /test -n "\$VAZAMENTO_TERMOS"/);
  const gitleaks = varredura.indexOf('name: Obter Gitleaks oficial fixado');
  assert.ok(varredura.indexOf('name: Recusar credenciais de aviso ausentes') < gitleaks, 'credenciais conferidas antes do download do Gitleaks');
  const marcador = varredura.split('\n      - ').find((st) => st.startsWith('name: Marcar varredura sem resumo'));
  assert.ok(marcador, 'marcador de falha sem resumo');
  assert.match(marcador, /^        if: \$\{\{ !cancelled\(\) \}\}\n/m);
  assert.match(marcador, /if \[ ! -s "\$VAZAMENTO_REPORT" \]/, 'marcador so quando o resumo nao foi gravado');
  assert.match(marcador, /"total":1/, 'marcador conta como ocorrencia para o aviso abrir issue');
  const notificar = jobs.notificar.join('\n');
  assert.match(notificar, /^    needs: varredura$/m);
  assert.match(notificar, /!cancelled\(\) && github\.ref == 'refs\/heads\/main' && needs\.varredura\.result == 'failure'/, 'aviso so depois de varredura reprovada na main');
  assert.ok(varredura.includes('name: vazamento-resumo-${{ github.run_attempt }}'), 'resumo nomeado pela tentativa do run');
  assert.ok(!/uses: actions\/download-artifact@/.test(semanal), 'download-artifact fora da politica de actions permitidas');
  assert.match(notificar, /^    permissions:\n      contents: read\n      actions: read$/m, 'leitura de actions no job consumidor');
  const baixar = notificar.split('\n      - ').find((st) => st.startsWith('name: Baixar o resumo do proprio run'));
  assert.ok(baixar, 'resumo baixado pelo gh do proprio run');
  assert.match(baixar, /GH_TOKEN: \$\{\{ github\.token \}\}/, 'token do proprio run');
  assert.match(baixar, /gh run download "\$GITHUB_RUN_ID" --repo "\$GITHUB_REPOSITORY" \\\n\s+--name "vazamento-resumo-\$GITHUB_RUN_ATTEMPT" --dir "\$destination"/, 'download escopado ao run e a tentativa');
  assert.match(baixar, /umask 077/);
  assert.match(baixar, /exit 1/, 'falha fechada quando o resumo nao chega');
  assert.ok(notificar.indexOf('name: Baixar o resumo do proprio run') < notificar.indexOf('name: Abrir issue'), 'aviso so depois do download');
  assert.match(notificar, /VAZAMENTO_ISSUE_REPO: \$\{\{ vars\.VAZAMENTO_ISSUE_REPO \}\}/, 'destino vem da variavel nomeada');
  assert.match(notificar, /VAZAMENTO_ISSUE_TOKEN: \$\{\{ secrets\.VAZAMENTO_ISSUE_TOKEN \}\}/, 'token vem do segredo nomeado');
  assert.ok(!/GITHUB_TOKEN/.test(semanal), 'sem token de escrita do repositorio');
  const segredos = [...semanal.matchAll(/\$\{\{[^}]*\bsecrets\.(\w+)/g)].map((m) => m[1]).sort();
  assert.deepEqual(segredos, ['VAZAMENTO_ISSUE_TOKEN', 'VAZAMENTO_ISSUE_TOKEN', 'VAZAMENTO_TERMOS', 'VAZAMENTO_TERMOS'], 'so os segredos nomeados');
});

// Sintético, sem rede e sem segredo real: o passo do workflow baixa o resumo do próprio run e
// tentativa com um gh de mentira, e o aviso só transporta contagens quando o destino é privado.
test('vazamento-semanal: download do proprio run e aviso falham fechados, sem conteudo de varredura', async () => {
  const roteiroDoPasso = (job, nome) => {
    const bloco = job.split('\n      - ').find((st) => st.startsWith(`name: ${nome}`));
    assert.ok(bloco, `passo ausente: ${nome}`);
    const linhas = bloco.split('\n');
    const inicio = linhas.indexOf('        run: |');
    assert.ok(inicio > 0, `${nome}: run em bloco`);
    const roteiro = [];
    for (const linha of linhas.slice(inicio + 1)) {
      if (!linha.startsWith('          ')) break;
      roteiro.push(linha.slice(10));
    }
    return roteiro.join('\n') + '\n';
  };
  const baixar = roteiroDoPasso(jobsDe(semanal).notificar.join('\n'), 'Baixar o resumo do proprio run sem expor conteudo');
  const bin = mkdtempSync(join(tmpdir(), 'vazamento-gh-'));
  const raiz = mkdtempSync(join(tmpdir(), 'vazamento-run-'));
  try {
    writeFileSync(join(bin, 'gh'), [
      '#!/usr/bin/env bash',
      'set -euo pipefail',
      'test "$1" = run',
      'test "$2" = download',
      'test "$3" = "$GITHUB_RUN_ID"',
      'destino=""',
      'nome=""',
      'anterior=""',
      'for argumento in "$@"; do',
      '  [ "$anterior" = --dir ] && destino="$argumento"',
      '  [ "$anterior" = --name ] && nome="$argumento"',
      '  anterior="$argumento"',
      'done',
      'test "$nome" = "vazamento-resumo-$GITHUB_RUN_ATTEMPT"',
      'test -n "$destino"',
      `test "\${STUB_GH:-}" = ok || { echo 'stub: artefato ausente' >&2; exit 1; }`,
      `printf '%s\\n' '{"total":2,"rules":{"termo-pessoal":2}}' > "$destino/vazamento-report.json"`,
      '',
    ].join('\n'), { mode: 0o755 });
    const executar = (stub, base) => spawnSync('bash', ['-e', '-o', 'pipefail', '-c', baixar], {
      env: {
        PATH: `${bin}:${process.env.PATH}`, HOME: bin, RUNNER_TEMP: base,
        GITHUB_RUN_ID: '424242', GITHUB_RUN_ATTEMPT: '2', GITHUB_REPOSITORY: 'fixture/repo',
        GH_TOKEN: 'ficticio', STUB_GH: stub,
      },
      encoding: 'utf8',
    });
    const baseOk = mkdtempSync(join(raiz, 'ok-'));
    const ok = executar('ok', baseOk);
    assert.equal(ok.status, 0, ok.stderr);
    const resumo = JSON.parse(readFileSync(join(baseOk, 'vazamento-resumo', 'vazamento-report.json'), 'utf8'));
    assert.equal(resumo.total, 2, 'resumo do proprio run e tentativa');
    const baseFalha = mkdtempSync(join(raiz, 'falha-'));
    const falha = executar('falha', baseFalha);
    assert.notEqual(falha.status, 0, 'sem resumo o job consumidor reprova');
    assert.match(`${falha.stdout}${falha.stderr}`, /Resumo da varredura indisponivel/, 'erro generico, sem conteudo');
    assert.ok(!readdirSync(join(baseFalha, 'vazamento-resumo')).includes('vazamento-report.json'), 'nada segue quando o download falha');

    const chamadas = [];
    const request = async (url, options) => { chamadas.push([url, options]); return { ok: true, json: async () => ({ private: true }) }; };
    await notify(resumo, { sourcePrivate: false, repository: 'fixture/public', target: 'fixture/private', token: 'ficticio' }, request);
    assert.equal(chamadas.length, 2, 'confere o destino privado e abre a issue');
    const corpo = JSON.parse(chamadas[1][1].body);
    assert.match(corpo.body, /termo-pessoal: 2/);
    assert.ok(!corpo.body.includes('Caminhos') && !corpo.body.includes('vazamento-resumo'), 'origem publica envia so contagens');
    chamadas.length = 0;
    await assert.rejects(notify(resumo, { sourcePrivate: false, repository: 'fixture/public', target: '', token: 'ficticio' }, request));
    assert.equal(chamadas.length, 0, 'sem destino privado confirmado nada e enviado');
  } finally {
    rmSync(bin, { recursive: true, force: true });
    rmSync(raiz, { recursive: true, force: true });
  }
});
