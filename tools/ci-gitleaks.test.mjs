import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, readFileSync, writeFileSync, mkdirSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
const script = resolve('tools/ci-gitleaks.mjs');
const scanner = process.env.GITLEAKS_BIN || 'gitleaks';
test('diff limpo, vazamento novo, supressões do PR, merge e saída sem segredo', () => {
  const dir = mkdtempSync(join(tmpdir(), 'test-ci-gitleaks-'));
  const git = (...args) => {
    const r = spawnSync('git', args, { cwd: dir, encoding: 'utf8' });
    assert.equal(r.status, 0, 'Git da fixture falhou');
    return r.stdout.trim();
  };
  const commit = () => { git('add', '.'); git('commit', '-qm', 'fixture'); return git('rev-parse', 'HEAD'); };
  const run = (base, head) => {
    const trustedMain = git('rev-parse', 'refs/remotes/origin/main');
    const trustedScript = join(dir, '.git', 'trusted-ci-gitleaks.mjs');
    writeFileSync(trustedScript, git('show', `${trustedMain}:tools/ci-gitleaks.mjs`));
    const eventPath = join(dir, '.git', 'event.json');
    writeFileSync(eventPath, JSON.stringify({ before: base, after: head }));
    return spawnSync(process.execPath, [trustedScript], { cwd: dir, encoding: 'utf8', env: { ...process.env, GITLEAKS_TRUSTED_MAIN_SHA: trustedMain, GITHUB_EVENT_NAME: 'push', GITHUB_EVENT_PATH: eventPath, GITHUB_STEP_SUMMARY: '', GITLEAKS_BIN: scanner } });
  };
  try {
    git('init', '-q'); git('config', 'user.name', 'Fixture'); git('config', 'user.email', 'fixture@example.invalid');
    writeFileSync(join(dir, 'README'), 'base\n');
    mkdirSync(join(dir, '.github/security'), { recursive: true });
    mkdirSync(join(dir, 'tools'));
    writeFileSync(join(dir, 'tools/ci-gitleaks.mjs'), readFileSync(script));
    writeFileSync(join(dir, '.github/security/gitleaks-baseline.fingerprints'), '');
    writeFileSync(join(dir, '.gitleaks.toml'), '[extend]\nuseDefault = true\n');
    const base = commit();
    git('update-ref', 'refs/remotes/origin/main', base);
    writeFileSync(join(dir, 'README'), 'sem segredo\n');
    const clean = commit();
    assert.equal(run(base, clean).status, 0);
    // Valor sintético, construído apenas dentro do repositório descartável.
    const secret = 'AKIA' + 'NQBGYFJPXCMVZDKR';
    writeFileSync(join(dir, 'fixture.txt'), `aws_access_key_id = "${secret}" # gitleaks:allow\n`);
    const leak = commit();
    const first = run(clean, leak);
    assert.equal(first.status, 1);
    assert.match(first.stdout, /aws-access-token/);
    assert.ok(!`${first.stdout}${first.stderr}`.includes(secret));
    writeFileSync(join(dir, '.github/security/gitleaks-baseline.fingerprints'), `${leak}:fixture.txt:aws-access-token:1\n`);
    writeFileSync(join(dir, '.gitleaksignore'), 'fixture.txt:aws-access-token:1\n');
    writeFileSync(join(dir, '.gitleaks.toml'), '[extend]\nuseDefault = true\ndisabledRules = ["aws-access-token"]\n');
    writeFileSync(join(dir, 'tools/ci-gitleaks.mjs'), 'process.exit(0);\n');
    const bypass = commit();
    assert.equal(run(clean, bypass).status, 1, 'script, configuração, baseline e .gitleaksignore do PR não podem suprimir o vazamento');
    git('checkout', '-qb', 'merge-base', clean);
    writeFileSync(join(dir, 'branch-a'), 'a');
    const branchA = commit();
    git('checkout', '-qb', 'branch-b', clean);
    writeFileSync(join(dir, 'branch-b'), 'b');
    commit();
    git('merge', '--no-ff', '--no-commit', 'merge-base');
    writeFileSync(join(dir, 'merge-only.txt'), `aws_access_key_id = "${secret}"\n`);
    const merge = commit();
    const merged = run(branchA, merge);
    assert.equal(merged.status, 1, 'segredo introduzido somente no merge precisa bloquear');
    assert.match(merged.stdout, /aws-access-token/);
    assert.ok(!`${merged.stdout}${merged.stderr}`.includes(secret));
    // Mesmo com uma base antiga permissiva, prevalece a política atual da main.
    git('checkout', '-qb', 'old-policy', base);
    writeFileSync(join(dir, '.gitleaks.toml'), '[extend]\nuseDefault = true\ndisabledRules = ["aws-access-token"]\n');
    const oldPolicy = commit();
    git('checkout', '-qb', 'approved-main');
    writeFileSync(join(dir, '.gitleaks.toml'), '[extend]\nuseDefault = true\n');
    const approvedMain = commit();
    git('update-ref', 'refs/remotes/origin/main', approvedMain);
    git('checkout', '-qb', 'old-base-pr', oldPolicy);
    writeFileSync(join(dir, 'new.txt'), `aws_access_key_id = "${secret}"\n`);
    const oldBasePr = commit();
    assert.equal(run(oldPolicy, oldBasePr).status, 1, 'main atual deve prevalecer sobre configuração da base antiga do PR');
  } finally { rmSync(dir, { recursive: true, force: true }); }
});

test('remerge ignora a main trazida por merge e allowlist só cobre hex na política', () => {
  const dir = mkdtempSync(join(tmpdir(), 'test-ci-gitleaks-'));
  const git = (...args) => {
    const r = spawnSync('git', args, { cwd: dir, encoding: 'utf8' });
    return r.status === 0 ? r.stdout.trim() : null;
  };
  const ok = (...args) => { const out = git(...args); assert.notEqual(out, null, 'Git da fixture falhou'); return out; };
  const commit = () => { ok('add', '.'); ok('commit', '-qm', 'fixture'); return ok('rev-parse', 'HEAD'); };
  const write = (path, content) => writeFileSync(join(dir, path), content);
  // Mesmo formato do CI em PR: base.sha/head.sha e merge-base calculado pelo executor.
  const scanPr = (baseSha, headSha) => {
    const trustedMain = ok('rev-parse', 'refs/remotes/origin/main');
    const trustedScript = join(dir, '.git', 'trusted-ci-gitleaks.mjs');
    writeFileSync(trustedScript, ok('show', `${trustedMain}:tools/ci-gitleaks.mjs`));
    const eventPath = join(dir, '.git', 'event.json');
    writeFileSync(eventPath, JSON.stringify({ pull_request: { base: { sha: baseSha }, head: { sha: headSha } } }));
    const r = spawnSync(process.execPath, [trustedScript], { cwd: dir, encoding: 'utf8', env: { ...process.env, GITLEAKS_TRUSTED_MAIN_SHA: trustedMain, GITHUB_EVENT_NAME: 'pull_request', GITHUB_EVENT_PATH: eventPath, GITHUB_STEP_SUMMARY: '', GITLEAKS_BIN: scanner } });
    const findings = r.stdout.split('\n').filter((l) => l.startsWith('{')).map((l) => JSON.parse(l));
    return { ...r, findings };
  };
  // Valores sintéticos, construídos apenas dentro do repositório descartável.
  const secret = 'AKIA' + 'NQBGYFJPXCMVZDKR';
  const policy = 'apps/desktop/resources/personal-runtime-policy-v1.json';
  const digest = createHash('sha256').update('fixture-donna').digest('hex');
  const fakeToken = 'Fk' + 'Zq8vT2mWx7RpL4nB9cYh3JdS6gK1uQe5';
  try {
    ok('init', '-q', '-b', 'main'); ok('config', 'user.name', 'Fixture'); ok('config', 'user.email', 'fixture@example.invalid');
    mkdirSync(join(dir, '.github/security'), { recursive: true });
    mkdirSync(join(dir, 'tools'));
    mkdirSync(join(dir, 'apps/desktop/resources'), { recursive: true });
    write('tools/ci-gitleaks.mjs', readFileSync(script));
    write('.gitleaks.toml', readFileSync(resolve('.gitleaks.toml')));
    write('.github/security/gitleaks-baseline.fingerprints', '');
    write('conflict.txt', 'linha original\n');
    const root = commit();
    // Main: segredo antigo já aceito no baseline e alteração que conflita com o PR.
    write('main-secret.txt', `aws_access_key_id = "${secret}"\n`);
    const mainLeak = commit();
    write('.github/security/gitleaks-baseline.fingerprints', `${mainLeak}:main-secret.txt:aws-access-token:1\n`);
    write('conflict.txt', 'linha da main\n');
    const main = commit();
    ok('update-ref', 'refs/remotes/origin/main', main);

    // a) merge da main sem resolução manual: nada novo.
    ok('checkout', '-qb', 'pr-a', root);
    write('pr-a.txt', 'sem segredo\n');
    commit();
    ok('merge', '-q', '--no-edit', '-X', 'theirs', 'main');
    const a = scanPr(main, ok('rev-parse', 'HEAD'));
    assert.equal(a.status, 0, 'conteúdo da main trazido por merge não é novidade do PR');
    assert.deepEqual(a.findings, []);

    // b) segredo introduzido na resolução manual de um conflito.
    ok('checkout', '-qb', 'pr-b', root);
    write('conflict.txt', 'linha do PR\n');
    commit();
    assert.equal(git('merge', '-q', 'main'), null, 'a fixture precisa de conflito real');
    write('conflict.txt', `aws_access_key_id = "${secret}"\n`);
    ok('add', '.'); ok('commit', '-qm', 'merge com resolução');
    const mergeB = ok('rev-parse', 'HEAD');
    const b = scanPr(main, mergeB);
    assert.equal(b.status, 1, 'evil merge precisa bloquear');
    // O parser do Gitleaks 8.30.1 não reconhece a linha "remerge CONFLICT" e rotula
    // arquivo em conflito como b/<caminho>; a detecção continua (falha fechada).
    assert.deepEqual(b.findings.map((f) => [f.arquivo, f.regra, f.commit]), [['b/conflict.txt', 'aws-access-token', mergeB]]);
    assert.ok(!`${b.stdout}${b.stderr}`.includes(secret));

    // c) segredo em commit normal do PR.
    ok('checkout', '-qb', 'pr-c', main);
    write('pr-c.txt', `aws_access_key_id = "${secret}"\n`);
    const leakC = commit();
    const c = scanPr(main, leakC);
    assert.equal(c.status, 1);
    assert.deepEqual(c.findings.map((f) => [f.arquivo, f.regra, f.commit]), [['pr-c.txt', 'aws-access-token', leakC]]);

    // d) hex na política é permitido; token não-hex no mesmo arquivo e hex fora dele, não.
    ok('checkout', '-qb', 'pr-d', main);
    write(policy, `{\n  "electron_sha256_key": "${digest}",\n  "electron_git_key": "${digest.slice(0, 40)}"\n}\n`);
    const hexOnly = commit();
    const d1 = scanPr(main, hexOnly);
    assert.equal(d1.status, 0, 'digest SHA-256 e SHA Git na política não são segredo');
    assert.deepEqual(d1.findings, []);
    write(policy, `{\n  "electron_sha256_key": "${digest}",\n  "electron_api_key": "${fakeToken}",\n  "aws_access_key_id": "${secret}"\n}\n`);
    write('other.json', `{\n  "electron_sha256_key": "${digest}"\n}\n`);
    write(`${policy}.bak`, `{\n  "electron_sha256_key": "${digest}"\n}\n`);
    const mixed = commit();
    const d2 = scanPr(main, mixed);
    assert.equal(d2.status, 1);
    assert.deepEqual(d2.findings.map((f) => [f.arquivo, f.linha, f.regra]).sort(), [
      [policy, 3, 'generic-api-key'],
      [policy, 4, 'aws-access-token'],
      [`${policy}.bak`, 2, 'generic-api-key'],
      ['other.json', 2, 'generic-api-key'],
    ]);
    assert.ok(![fakeToken, secret].some((v) => `${d2.stdout}${d2.stderr}`.includes(v)));

    // Octopus: remerge pularia o merge; o executor precisa recusar o intervalo.
    ok('checkout', '-qb', 'oct-1', main); write('oct-1.txt', '1\n'); commit();
    ok('checkout', '-qb', 'oct-2', main); write('oct-2.txt', '2\n'); commit();
    ok('checkout', '-qb', 'pr-octopus', main); write('oct-0.txt', '0\n'); commit();
    ok('merge', '-q', '--no-ff', '--no-commit', 'oct-1', 'oct-2');
    write('octopus-only.txt', `aws_access_key_id = "${secret}"\n`);
    const octopus = commit();
    assert.equal(ok('rev-list', '--parents', '-1', octopus).split(' ').length, 4, 'a fixture precisa de octopus real');
    const o = scanPr(main, octopus);
    assert.notEqual(o.status, 0, 'octopus não pode passar vazio');
    assert.match(o.stderr, /merge octopus não suportado/);
    assert.deepEqual(o.findings, []);
    assert.ok(!`${o.stdout}${o.stderr}`.includes(secret));
  } finally { rmSync(dir, { recursive: true, force: true }); }
});

test('main sem baseline: nada é suprimido e a política continua vindo da main', () => {
  const dir = mkdtempSync(join(tmpdir(), 'test-ci-gitleaks-'));
  const git = (...args) => {
    const r = spawnSync('git', args, { cwd: dir, encoding: 'utf8' });
    assert.equal(r.status, 0, 'Git da fixture falhou');
    return r.stdout.trim();
  };
  const commit = () => { git('add', '.'); git('commit', '-qm', 'fixture'); return git('rev-parse', 'HEAD'); };
  const run = (base, head) => {
    const trustedMain = git('rev-parse', 'refs/remotes/origin/main');
    const eventPath = join(dir, '.git', 'event.json');
    writeFileSync(eventPath, JSON.stringify({ pull_request: { base: { sha: base }, head: { sha: head } } }));
    return spawnSync(process.execPath, [script], { cwd: dir, encoding: 'utf8', env: { ...process.env, GITLEAKS_TRUSTED_MAIN_SHA: trustedMain, GITHUB_EVENT_NAME: 'pull_request', GITHUB_EVENT_PATH: eventPath, GITHUB_STEP_SUMMARY: '', GITLEAKS_BIN: scanner } });
  };
  const secret = 'AKIA' + 'NQBGYFJPXCMVZDKR';
  try {
    git('init', '-q', '-b', 'main'); git('config', 'user.name', 'Fixture'); git('config', 'user.email', 'fixture@example.invalid');
    writeFileSync(join(dir, '.gitleaks.toml'), '[extend]\nuseDefault = true\n');
    const base = commit();
    git('update-ref', 'refs/remotes/origin/main', base);
    writeFileSync(join(dir, 'README'), 'sem segredo\n');
    const clean = commit();
    assert.equal(run(base, clean).status, 0);
    mkdirSync(join(dir, '.github/security'), { recursive: true });
    writeFileSync(join(dir, 'fixture.txt'), `aws_access_key_id = "${secret}"\n`);
    const leak = commit();
    writeFileSync(join(dir, '.github/security/gitleaks-baseline.fingerprints'), `${leak}:fixture.txt:aws-access-token:1\n`);
    const suppressed = commit();
    const r = run(base, suppressed);
    assert.equal(r.status, 1, 'baseline criada pelo PR não suprime nada enquanto a main não a tem');
    assert.match(r.stdout, /aws-access-token/);
    assert.ok(!`${r.stdout}${r.stderr}`.includes(secret));
    git('rm', '-q', '.gitleaks.toml');
    const noPolicy = commit();
    git('update-ref', 'refs/remotes/origin/main', noPolicy);
    assert.notEqual(run(noPolicy, noPolicy).status, 0, 'sem .gitleaks.toml na main a varredura é recusada');
  } finally { rmSync(dir, { recursive: true, force: true }); }
});
