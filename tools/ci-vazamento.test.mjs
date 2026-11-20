import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, readFileSync, writeFileSync, rmSync, symlinkSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { fingerprint, publicRules, scanLine, scan, maskTerms, safePath, loadPolicy, notify, termExceptions } from './ci-vazamento.mjs';

const policy = JSON.parse(readFileSync(new URL('../.github/security/vazamento.json', import.meta.url), 'utf8'));
const script = resolve('tools/ci-vazamento.mjs');
const personalPath = (name = 'pessoa-ficticia') => '/Users/' + name + '/projeto';
const privateAddress = (suffix = '7') => ['192', '168', '24', suffix].join('.');
const fakeService = 'whsec_' + 'FICTICIO987654321';
const vault = 'op:' + '//cofre-ficticio/item-ficticio';
const ids = (line) => publicRules(line, policy);

for (const [rule, bad, good] of [
  ['caminho-pessoal', [personalPath(), 'C:' + '\\Users\\pessoa-ficticia\\projeto'], ['/Users/you/test', '/home/example/.config/app.json', '/Users/exemplo/test', '/Users/<usuario>/test', '/Users/$USER/test', '/Users/Shared/test', '~/test']],
  ['ip-privado', [privateAddress(), '10.' + '8.0.1', '172.' + '16.0.1', '100.' + '64.0.1', '169.' + '254.0.1', 'fc' + '00::1', 'fd' + '12::1', 'fe' + '80::1', 'fe' + 'bf::1'], ['127.0.0.1', '::1', '192.0.2.1', '198.51.100.9', '203.0.113.8', '2001:db8::1', '172.15.0.1', '172.32.0.1', '100.63.0.1', '100.128.0.1', 'fec0::1']],
  ['host-interno', ['https://node-ficticio.' + 'ts.net', 'https://node-ficticio.' + 'local', 'hostname = node-ficticio.' + 'local', '"hostname": "node-ficticio.' + 'local"', 'Host ' + 'ssh-ficticio'], ['https://example.invalid', 'import "./arquivo.local"', 'Host <alias>', 'Host *']],
  ['id-servico', [fakeService, 'pk_' + 'live_FICTICIO987', 'sk_' + 'test_FICTICIO987', 'ins_' + 'FICTICIO12345678901', 'user_' + 'FICTICIO12345678901', 'clerk.' + 'instancia-ficticia.invalid', 'conta-ficticia.' + 'workers.dev', 'account_id = "' + '123456789abcdef0123456789abcdef0' + '"', 'database_id = "' + '12345678-1234-1234-1234-123456789abc' + '"', 'kv_namespaces = [{ id = "' + '123456789abcdef0123456789abcdef0' + '" }]'], ['pk_live_<chave>', 'whsec_<chave>', 'account_id = "00000000000000000000000000000000"', 'database_id = "00000000-0000-0000-0000-000000000000"', 'example.workers.dev']],
  ['cofre', [vault], ['referência de cofre', 'https://example.invalid']],
]) {
  test(`${rule}: detecta valores fictícios e aceita exceções públicas`, () => {
    for (const line of bad) assert.ok(ids(line).includes(rule), `caso positivo da regra ${rule}`);
    for (const line of good) assert.ok(!ids(line).includes(rule), `caso negativo da regra ${rule}`);
    for (const line of bad) assert.equal(ids(`${line} # vazamento-ok: exemplo fictício`).includes(rule), !policy.rules.find((r) => r.id === rule).marker);
  });
}

test('marcador exige motivo, nunca libera termos, serviços ou cofre', () => {
  assert.ok(ids(`${privateAddress()} vazamento-ok: `).includes('ip-privado'));
  for (const text of [fakeService, vault]) assert.equal(ids(`${text} vazamento-ok: exemplo`).length, 1);
  assert.ok(scanLine('fixture', 1, 'cacto ficticio vazamento-ok: exemplo', policy, ['Cácto Fictício']).some((f) => f.rule === 'termo-pessoal'));
});

test('termos sem caixa/acento e saída sanitizada inclusive caminhos', () => {
  const commands = [];
  const terms = maskTerms('Cácto Fictício\nAgave Inventado\n', (s) => commands.push(s));
  const events = [...commands];
  const rows = scanLine('CACTO FICTICIO.txt', 9, 'segredo: cAcTo FiCtIcIo e linha privada inteira', policy, terms);
  for (const f of rows) events.push(`${safePath(f.path, terms, policy)}:${f.line} regra=${f.rule}`);
  assert.equal(rows.length, 1);
  assert.ok(events[0].startsWith('::add-mask::'));
  assert.equal(events.findIndex((s) => !s.startsWith('::add-mask::')), commands.length);
  const output = events.slice(commands.length).join('\n');
  assert.ok(!output.toLowerCase().includes('cacto ficticio'));
  assert.ok(!output.includes('Cácto Fictício'));
  assert.ok(!output.includes('linha privada inteira'));
  assert.equal(safePath(personalPath(), terms, policy), '[caminho-omitido]');
  assert.equal(safePath('bad\n::warning::x', terms, policy), '[caminho-omitido]');
});

function fixture(fn) {
  const cwd = mkdtempSync(join(tmpdir(), 'd519-test-'));
  const git = (...args) => {
    const r = spawnSync('git', args, { cwd, encoding: 'utf8', env: { PATH: '/usr/bin:/bin:/usr/local/bin', HOME: cwd, GIT_CONFIG_NOSYSTEM: '1', GIT_CONFIG_GLOBAL: '/dev/null' } });
    assert.equal(r.status, 0, 'Git da fixture não concluiu');
    return r.stdout.trim();
  };
  const write = (path, data) => { mkdirSync(join(cwd, path, '..'), { recursive: true }); writeFileSync(join(cwd, path), data); };
  const commit = (message = 'fixture') => { git('add', '.'); git('-c', 'core.hooksPath=/dev/null', 'commit', '-qm', message); return git('rev-parse', 'HEAD'); };
  try {
    git('init', '-q', '-b', 'main'); git('config', 'user.name', 'Fixture'); git('config', 'user.email', 'fixture@example.invalid');
    write('README', 'início\n');
    write('.github/security/vazamento.json', JSON.stringify(policy));
    write('.github/security/vazamento-inventario.json', JSON.stringify({ fingerprints: [] }));
    write('tools/ci-vazamento.mjs', readFileSync(script));
    const base = commit(); git('update-ref', 'refs/remotes/origin/main', base);
    fn({ cwd, git, write, commit, base });
  } finally { rmSync(cwd, { recursive: true, force: true }); }
}

test('pr: só linhas adicionadas e mensagens; existência e remoção não acusam', () => fixture(({ cwd, write, commit, base }) => {
  write('documento.md', `${privateAddress()}\ntexto original\n`);
  const old = commit();
  write('documento.md', `${privateAddress()}\ntexto alterado\n`);
  const change = commit();
  const run = (a, b) => scan({ cwd, mode: 'pr', base: a, head: b, policy }).findings;
  assert.equal(run(old, change).length, 0);
  write('documento.md', `${privateAddress()}\n${vault}\n`);
  const added = commit();
  assert.deepEqual(run(change, added).map((f) => f.rule), ['cofre']);
  write('documento.md', 'texto sem ocorrência\n');
  const removed = commit();
  assert.equal(run(added, removed).length, 0);
  write('mais.md', 'limpo\n');
  const message = commit(`fixture ${fakeService}`);
  assert.ok(run(removed, message).some((f) => f.rule === 'id-servico' && f.path.startsWith('commit/')));
  assert.ok(run(base, removed).some((f) => f.rule === 'id-servico') === false);
}));

test('inventário: árvore ignora impressão conhecida, pr sempre acusa', () => fixture(({ cwd, write, commit, base }) => {
  write('documento.md', `${privateAddress()}\n`);
  const head = commit();
  const inventory = [fingerprint('ip-privado', 'documento.md', privateAddress())];
  assert.equal(scan({ cwd, mode: 'arvore', head, policy, inventory }).findings.filter((f) => f.path === 'documento.md').length, 0);
  assert.equal(scan({ cwd, mode: 'pr', base, head, policy, inventory }).findings.filter((f) => f.path === 'documento.md').length, 1);
  assert.equal(fingerprint('ip-privado', 'x', ' ÁGAVE   X '), fingerprint('ip-privado', 'x', 'agave x'));
}));

test('marcador não dispensa regra em nome de arquivo, mesmo sem outro achado', () => fixture(({ cwd, write, commit, base }) => {
  const name = 'capturas/Users/pessoa-ficticia/vazamento-ok: fixture.txt';
  write(name, 'texto limpo\n');
  const head = commit();
  const hits = scan({ cwd, mode: 'pr', base, head, policy }).findings.filter((f) => f.path === name);
  assert.deepEqual(hits.map((f) => f.rule), ['caminho-pessoal']);
  assert.equal(publicRules('/Users/pessoa-ficticia/x vazamento-ok: exemplo', policy).length, 0, 'em conteúdo o marcador continua valendo');
}));

test('B6: atributos Git não escondem texto de commit intermediário', () => fixture(({ cwd, write, commit, base }) => {
  write('.gitattributes', 'nota.txt -diff\n');
  write('nota.txt', `${vault}\n`);
  commit('intermediario');
  write('nota.txt', 'texto limpo\n');
  const head = commit('limpa');
  const rules = scan({ cwd, mode: 'pr', base, head, policy }).findings.map((f) => `${f.path}:${f.rule}`);
  assert.ok(rules.includes('nota.txt:cofre'), rules.join());
}));

test('B8: troca de tipo em commit intermediário (arquivo para link) também é varrida', () => fixture(({ cwd, write, commit, base }) => {
  write('atalho', 'texto limpo\n'); commit('arquivo');
  rmSync(join(cwd, 'atalho')); symlinkSync(personalPath(), join(cwd, 'atalho')); commit('vira link');
  rmSync(join(cwd, 'atalho')); write('atalho', 'texto limpo\n');
  const head = commit('volta a arquivo');
  const rules = scan({ cwd, mode: 'pr', base, head, policy }).findings.map((f) => `${f.path}:${f.rule}`);
  assert.ok(rules.includes('atalho:caminho-pessoal'), rules.join());
}));

test('B7: conteúdo trazido só pela resolução de um merge intermediário é varrido; octopus é recusado', () => fixture(({ cwd, git, write, commit, base }) => {
  // Máquinas com wrapper de git podem gravar log dentro do HOME da fixture; fica fora das trocas de branch.
  write('.git/info/exclude', '.capy/\n'); git('rm', '-r', '-q', '--cached', '--ignore-unmatch', '.capy');
  git('-c', 'core.hooksPath=/dev/null', 'commit', '-q', '--allow-empty', '-m', 'prepara');
  const start = git('rev-parse', 'HEAD');
  git('checkout', '-q', '-b', 'lado'); write('lado.md', 'lado\n'); commit('lado');
  git('checkout', '-q', 'main'); write('outro.md', 'outro\n'); commit('main2');
  git('merge', '-q', '--no-ff', '--no-commit', 'lado');
  write('resolucao.md', `${vault}\n`);
  commit('merge com resolucao');
  write('resolucao.md', 'texto limpo\n');
  const head = commit('limpa');
  const found = scan({ cwd, mode: 'pr', base, head, policy }).findings;
  assert.ok(found.some((f) => f.path === 'resolucao.md' && f.rule === 'cofre'), found.map((f) => `${f.path}:${f.rule}`).join());
  assert.ok(!found.some((f) => f.path === 'lado.md' || f.path === 'outro.md'), 'conteúdo limpo dos pais não vira achado');
  git('checkout', '-q', '-b', 'a', start); write('a.md', 'a\n'); commit('a');
  git('checkout', '-q', '-b', 'b', start); write('b.md', 'b\n'); commit('b');
  git('checkout', '-q', 'main'); git('merge', '-q', '--no-ff', '-m', 'octopus', 'a', 'b');
  const octo = git('rev-parse', 'HEAD');
  assert.throws(() => scan({ cwd, mode: 'pr', base, head: octo, policy }), /mais de dois pais/);
}));

test('binários com NUL ficam fora; arquivos sensíveis geram achado sem ler conteúdo', () => fixture(({ cwd, write, commit }) => {
  write('imagem.png', Buffer.from([0, 1, 2, 3]));
  // Valores fictícios, sem consultar arquivos ou ambiente da máquina.
  write('.env.fixture', vault); write('private.key', vault);
  const head = commit();
  const result = scan({ cwd, mode: 'arvore', head, policy });
  assert.deepEqual(result.limits, { binary: 1, sensitive: 2, submodule: 0 });
  assert.ok(!result.findings.some((f) => f.path === 'imagem.png'));
  assert.deepEqual(result.findings.filter((f) => ['.env.fixture', 'private.key'].includes(f.path)).map((f) => [f.path, f.rule, f.line]), [['.env.fixture', 'arquivo-sensivel', undefined], ['private.key', 'arquivo-sensivel', undefined]]);
  assert.ok(!result.findings.some((f) => ['.env.fixture', 'private.key'].includes(f.path) && f.rule === 'cofre'));
}));

function cli({ cwd, base, head, fork = false, author = 'fixture', authorType = author === 'dependabot[bot]' ? 'Bot' : 'User', eventName = 'pull_request', terms = '', privateRepo = true, mode = 'pr', gitleaksBin = '', reportPath = '' }) {
  const eventPath = join(cwd, '.git/event.json');
  writeFileSync(eventPath, JSON.stringify({ repository: { private: privateRepo, full_name: 'fixture/repo' }, pull_request: { user: { login: author, type: authorType }, base: { sha: base }, head: { sha: head, repo: { fork, full_name: fork ? 'fixture/fork' : 'fixture/repo' } } } }));
  // Construído do zero: não lê, copia ou enumera o ambiente do operador.
  return spawnSync(process.execPath, [script, mode], { cwd, encoding: 'utf8', env: {
    PATH: '/usr/bin:/bin:/usr/local/bin', HOME: cwd, GITHUB_EVENT_PATH: eventPath, GITHUB_EVENT_NAME: eventName,
    VAZAMENTO_TRUSTED_MAIN_SHA: base, VAZAMENTO_TERMOS: terms, GITLEAKS_BIN: gitleaksBin, VAZAMENTO_REPORT: reportPath,
  } });
}

test('CLI: fork sem secret usa regras públicas; fora do fork falha fechado', () => fixture((ctx) => {
  const { write, commit, base } = ctx;
  write('novo.md', 'texto limpo\n'); const head = commit();
  const fork = cli({ ...ctx, head, fork: true });
  assert.equal(fork.status, 0); assert.match(fork.stdout, /termos pessoais: não verificados \(fork\)/);
  assert.notEqual(cli({ ...ctx, head }).status, 0);
  write('novo.md', `${vault}\n`); const bad = commit();
  assert.equal(cli({ ...ctx, head: bad, fork: true }).status, 1);
  assert.equal(base.length, 40);
}));

test('CLI: instalação inicial sem secret usa regras públicas; depois disso o secret volta a ser obrigatório', () => fixture((ctx) => {
  const { git, write, commit } = ctx;
  git('rm', 'tools/ci-vazamento.mjs'); git('rm', '.github/security/vazamento.json'); git('rm', '.github/security/vazamento-inventario.json');
  const before = commit(); git('update-ref', 'refs/remotes/origin/main', before);
  write('tools/ci-vazamento.mjs', readFileSync(script));
  write('.github/security/vazamento.json', JSON.stringify(policy));
  write('.github/security/vazamento-inventario.json', JSON.stringify({ fingerprints: [] }));
  write('novo.md', 'texto limpo\n');
  const first = commit();
  const r = cli({ ...ctx, base: before, head: first });
  assert.equal(r.status, 0); assert.match(r.stdout, /termos pessoais: não verificados \(instalação inicial\)/);
  git('update-ref', 'refs/remotes/origin/main', first);
  write('novo.md', 'outro texto limpo\n'); const second = commit();
  assert.notEqual(cli({ ...ctx, base: first, head: second }).status, 0);
}));

test('CLI: mask vem antes de qualquer saída; termo e linha não escapam após máscaras', () => fixture((ctx) => {
  ctx.write('novo.md', 'prefixo cAcTo FiCtIcIo sufixo confidencial\n');
  const head = ctx.commit();
  const r = cli({ ...ctx, head, terms: 'Cácto Fictício' });
  assert.equal(r.status, 1);
  const lines = r.stdout.trim().split('\n');
  assert.ok(lines[0].startsWith('::add-mask::'));
  const output = lines.filter((l) => !l.startsWith('::add-mask::')).join('\n') + r.stderr;
  assert.ok(!/c[aá]cto fict[ií]cio/i.test(output)); assert.ok(!output.includes('sufixo confidencial'));
  assert.match(output, /regra=termo-pessoal/);
}));

test('política vem da main, alteração do PR não a relaxa; bootstrap é só na introdução', () => fixture(({ cwd, git, write, commit, base }) => {
  write('.github/security/vazamento.json', JSON.stringify({ ...policy, rules: [] }));
  const head = commit();
  assert.equal(loadPolicy(cwd, base, head, true).policy.rules.length, policy.rules.length);
  assert.throws(() => loadPolicy(cwd, head, head, true));
  git('rm', 'tools/ci-vazamento.mjs'); git('rm', '.github/security/vazamento.json'); git('rm', '.github/security/vazamento-inventario.json');
  const before = commit(); git('update-ref', 'refs/remotes/origin/main', before);
  write('tools/ci-vazamento.mjs', readFileSync(script));
  write('.github/security/vazamento.json', JSON.stringify(policy));
  write('.github/security/vazamento-inventario.json', JSON.stringify({ fingerprints: [] }));
  const first = commit();
  assert.equal(loadPolicy(cwd, before, first, true).policy.rules.length, policy.rules.length);
  assert.throws(() => loadPolicy(cwd, before, first, false));
}));

test('issue contém só contagens e caminhos; nunca abre em destino público', async () => {
  const report = { total: 2, rules: { 'cofre': 2 }, paths: ['docs/fixture.md'] };
  const calls = [];
  const request = async (url, options) => { calls.push([url, options]); return { ok: true, json: async () => ({ private: true }) }; };
  await notify(report, { sourcePrivate: true, repository: 'fixture/private', token: 'ficticio' }, request);
  assert.equal(calls.length, 2);
  const body = JSON.parse(calls[1][1].body).body;
  assert.match(body, /cofre: 2/); assert.match(body, /docs\/fixture.md/); assert.ok(!body.includes(vault));
  calls.length = 0;
  await assert.rejects(notify(report, { sourcePrivate: false, repository: 'fixture/public', token: 'ficticio' }, request));
  assert.equal(calls.length, 0);
  await assert.rejects(notify(report, { sourcePrivate: true, repository: 'fixture/public', token: 'ficticio' }, async () => ({ ok: true, json: async () => ({ private: false }) })));
  await notify(report, { sourcePrivate: false, repository: 'fixture/public', target: 'fixture/private', token: 'ficticio' }, request);
  assert.equal(calls.length, 2);
  assert.ok(!JSON.parse(calls[1][1].body).body.includes('docs/fixture.md'), 'origem pública não transporta caminhos à issue');
  calls.length = 0;
  await notify({ total: 2, rules: { cofre: 2 } }, { sourcePrivate: false, repository: 'fixture/public', target: 'fixture/private', token: 'ficticio' }, request);
  assert.match(JSON.parse(calls[1][1].body).body, /cofre: 2/);
});

test('contrato do workflow: scanner aprovado, testes sem secret e ci-ok obrigatório', () => {
  const ci = readFileSync(new URL('../.github/workflows/ci.yml', import.meta.url), 'utf8');
  const job = ci.split('  vazamento:\n')[1].split('\n  dependency-audit:')[0];
  assert.match(job, /needs: \[route, secrets\]/);
  assert.match(job, /permissions:\n      contents: read/);
  assert.ok(!job.includes('issues: write'));
  assert.match(job, /git show "\$source:tools\/ci-vazamento.mjs"/);
  assert.match(job, /test "\$GITHUB_EVENT_NAME" = pull_request/);
  assert.ok(!job.includes('node --test'), 'VM do secret não executa testes do PR');
  const testJob = ci.split('  ferramentas-ci:\n')[1].split('  hub:\n')[0];
  assert.ok(testJob.includes('tools/ci-vazamento.test.mjs'));
  assert.ok(!testJob.includes('secrets.'), 'VM dos testes não recebe secrets');
  assert.ok(!testJob.includes('VAZAMENTO_TERMOS'));
  assert.match(ci, /needs: \[route, secrets, vazamento, semgrep, changes,/);
  assert.match(ci, /\.vazamento.result == "success"/);
});

test('arquivos novos da D519 não introduzem padrões públicos reais', () => {
  const paths = ['tools/ci-vazamento.mjs', 'tools/ci-vazamento.test.mjs', '.github/security/vazamento.json', '.github/workflows/ci.yml', 'docs/guides/ci.md'];
  for (const path of paths) {
    const lines = readFileSync(resolve(path), 'utf8').split('\n');
    const hits = lines.flatMap((line, i) => publicRules(line, policy).map((rule) => `${path}:${i + 1} regra=${rule}`));
    assert.deepEqual(hits, [], 'fixtures e docs devem usar placeholders ou valores construídos');
  }
});

test('arvore exige inventário público vazio; histórico Gitleaks usa --all sem vazar relatório bruto', () => fixture((ctx) => {
  const { cwd, git, write, commit } = ctx;
  // Executor fictício: apenas confirma flags e devolve relatório sintético.
  const stub = join(cwd, '.git/gitleaks-fixture');
  writeFileSync(stub, `#!${process.execPath}\nconst fs = require('node:fs');\nconst args = process.argv.slice(2);\nif (!args.includes('--all') || !args.includes('--ignore-gitleaks-allow') || !args.includes('--redact=100')) process.exit(2);\nfs.writeFileSync(args[args.indexOf('--report-path') + 1], JSON.stringify([{File:'fixture-history.md',StartLine:4,Secret:'NAO_DEVE_SAIR',RuleID:'fixture'}]));\nconsole.log('NAO_DEVE_SAIR'); process.exit(1);\n`, { mode: 0o700 });
  write('.gitleaks.toml', '[extend]\nuseDefault = true\n');
  write('.github/security/gitleaks-baseline.fingerprints', '');
  write('.github/security/vazamento-inventario.json', JSON.stringify({ fingerprints: ['a'.repeat(64)] }));
  const base = commit(); git('update-ref', 'refs/remotes/origin/main', base);
  write('new.md', 'limpo\n'); const head = commit();
  const reportPath = join(cwd, '.git/safe-report.json');
  const r = cli({ ...ctx, base, head, mode: 'arvore', terms: 'Agave Fictício', gitleaksBin: stub, reportPath });
  assert.equal(r.status, 1);
  assert.ok(!`${r.stdout}${r.stderr}`.includes('NAO_DEVE_SAIR'));
  const report = JSON.parse(readFileSync(reportPath, 'utf8'));
  assert.equal(report.rules.gitleaks, 1);
  assert.deepEqual(Object.keys(report).sort(), ['limits', 'paths', 'rules', 'total']);
  assert.ok(!JSON.stringify(report).includes('NAO_DEVE_SAIR'));
  assert.notEqual(cli({ ...ctx, base, head, mode: 'arvore', terms: 'Agave Fictício', privateRepo: false }).status, 0);
}));

test('atributo -diff do PR não esconde texto adicionado', () => fixture(({ cwd, write, commit, base }) => {
  write('.gitattributes', '*.md -diff\n');
  write('novo.md', `${vault}\n`);
  const head = commit();
  const result = scan({ cwd, mode: 'pr', base, head, policy });
  assert.ok(result.findings.some((f) => f.path === 'novo.md' && f.rule === 'cofre'));
}));

test('D523: só prefixos exatos em URL excluem termos; separadores são equivalentes', () => {
  const terms = ['Cácto Fictício'];
  const p = { ...policy, urlsPermitidas: ['github.com/cacto-ficticio/', 'git@github.com:cacto-ficticio/', 'cacto-ficticio.github.io'] };
  const hits = (line) => scanLine('fixture', 1, line, p, terms).some((f) => f.rule === 'termo-pessoal');
  for (const line of ['https://github.com/cacto-ficticio/repo', 'github.com/cacto-ficticio/repo', 'git@github.com:cacto-ficticio/repo', 'https://cacto-ficticio.github.io/guia', 'https://cacto-ficticio.github.io', 'https://cacto-ficticio.github.io:443/guia']) assert.equal(hits(line), false, 'URL exata permitida');
  for (const line of ['Cácto Fictício', 'cacto-ficticio', 'cacto.ficticio', 'cacto_ficticio', 'https://github.com/outra-cacto-ficticio/repo', 'https://github.com/cacto-ficticio-extra/repo', 'https://cacto-ficticio.github.io.evil.invalid', 'https://cacto-ficticioXgithubYio', 'https://evil.invalid/github.com/cacto-ficticio/repo', 'https://evil.github.com/cacto-ficticio/repo', 'github.com/cacto-ficticio/repo Cacto Ficticio', 'https://github.com/cacto-ficticio/cacto-ficticio', 'https://cacto-ficticio.github.io@evil.invalid', 'https://cacto-ficticio.github.io:senha@evil.invalid', 'https://cacto-ficticio.github.io+evil.invalid']) assert.equal(hits(line), true, 'fora do prefixo exato continua bloqueado');
  assert.ok(scanLine('fixture', 1, 'https://github.com/cacto-ficticio/repo ' + vault, p, terms).some((f) => f.rule === 'cofre'), 'exceção de URL não suprime regras públicas');
});

test('CLI: arvore pública gera só contagens; privada mantém caminhos sanitizados', () => fixture((ctx) => {
  ctx.write('documento-publico.md', `${vault}\n`);
  const head = ctx.commit();
  const reportPath = join(ctx.cwd, '.git/report-public.json');
  const r = cli({ ...ctx, head, mode: 'arvore', privateRepo: false, terms: 'Agave Fictício', reportPath });
  assert.equal(r.status, 1);
  const report = JSON.parse(readFileSync(reportPath, 'utf8'));
  assert.deepEqual(Object.keys(report).sort(), ['rules', 'total']);
  assert.equal(report.rules.cofre, 1);
  assert.ok(!r.stdout.includes('documento-publico.md'));
  assert.ok(!Object.hasOwn(report, 'paths'));
  assert.ok(!/\.(?:md|mjs|json):\d+/.test(r.stdout));
  const privateReport = join(ctx.cwd, '.git/report-private.json');
  const privateResult = cli({ ...ctx, head, mode: 'arvore', terms: 'Agave Fictício', reportPath: privateReport });
  assert.equal(privateResult.status, 1);
  assert.ok(privateResult.stdout.includes('documento-publico.md'));
  assert.ok(JSON.parse(readFileSync(privateReport, 'utf8')).paths.includes('documento-publico.md'));
}));

test('arquivo sensível novo no PR falha sem ler seu conteúdo', () => fixture((ctx) => {
  ctx.write('.env', `${vault}\n`);
  const head = ctx.commit();
  const r = cli({ ...ctx, head, fork: true });
  assert.equal(r.status, 1);
  assert.match(r.stdout, /regra=arquivo-sensivel/);
  assert.ok(!r.stdout.includes('regra=cofre'));
  assert.ok(!r.stdout.includes(vault));
  assert.ok(!r.stdout.includes('undefined'));
}));

test('UTF-8 inválido não esconde referência de cofre em pr ou arvore', () => fixture(({ cwd, write, commit, base }) => {
  write('texto-malformado.txt', Buffer.concat([Buffer.from([0xff]), Buffer.from(`\n${vault}\n`)]));
  const head = commit();
  for (const mode of ['pr', 'arvore']) {
    const result = scan({ cwd, mode, base, head, policy });
    assert.ok(result.findings.some((f) => f.path === 'texto-malformado.txt' && f.rule === 'cofre'));
    assert.equal(result.limits.binary, 0);
  }
}));


test('A1: sem secret nenhum autor passa, Dependabot incluído; com a lista o Dependabot é verificado', () => fixture((ctx) => {
  ctx.write('novo.md', 'texto limpo\n');
  const head = ctx.commit();
  assert.equal(cli({ ...ctx, head, author: 'dependabot[bot]', terms: 'Cácto Fictício' }).status, 0);
  for (const author of ['dependabot[bot]', 'fixture', 'outro[bot]', 'dependabot']) {
    assert.equal(cli({ ...ctx, head, author }).status, 2);
  }
  for (const eventName of ['push', 'merge_group', 'workflow_dispatch', 'schedule']) {
    assert.equal(cli({ ...ctx, head, author: 'dependabot[bot]', eventName }).status, 2);
  }
  ctx.write('novo.md', `${vault}\n`);
  const bad = ctx.commit();
  assert.equal(cli({ ...ctx, head: bad, author: 'dependabot[bot]', terms: 'Cácto Fictício' }).status, 1);
  ctx.write('novo.md', 'Cácto Fictício\n');
  const personal = ctx.commit();
  assert.equal(cli({ ...ctx, head: personal, author: 'dependabot[bot]', terms: 'Cácto Fictício' }).status, 1);
}));

test('A2: caminhos Windows escapados mantêm detecção e exceções', () => {
  const escaped = (user) => 'C:' + String.fromCharCode(92).repeat(2) + 'Users' + String.fromCharCode(92).repeat(2) + user + String.fromCharCode(92).repeat(2) + 'projeto';
  assert.ok(ids(escaped('pessoa-ficticia')).includes('caminho-pessoal'));
  for (const user of ['you', '<usuario>', '${USER}']) assert.ok(!ids(escaped(user)).includes('caminho-pessoal'));
  assert.ok(!ids(escaped('pessoa-ficticia') + ' # vazamento-ok: fixture sintética').includes('caminho-pessoal'));
});

test('A2: caminhos Linux mantêm detecção e exceções', () => {
  const linux = (user) => '/home/' + user + '/projeto';
  assert.ok(ids(linux('pessoa-ficticia')).includes('caminho-pessoal'));
  for (const user of ['user', '<usuario>', '$USER']) assert.ok(!ids(linux(user)).includes('caminho-pessoal'));
  assert.equal(safePath(linux('pessoa-ficticia'), [], policy), '[caminho-omitido]');
  assert.equal(safePath('capturas/Users/pessoa-ficticia/vazamento-ok: fixture.txt', [], policy), '[caminho-omitido]');
});

test('A3: IPv4 privado no fim de frase é detectado sem aceitar octeto extra', () => {
  assert.ok(ids(`Destino: ${privateAddress()}.`).includes('ip-privado'));
  for (const line of [privateAddress() + '.8', 'x' + privateAddress(), '192.0.2.1.']) assert.ok(!ids(line).includes('ip-privado'));
});

test('A4: primeiro rótulo Clerk reconhece placeholders sem liberar instância', () => {
  for (const domain of ['example.invalid', 'example.com', 'exemplo.invalid']) assert.ok(!ids('clerk.' + domain).includes('id-servico'));
  for (const domain of ['example-extra.invalid', 'instancia-ficticia.invalid']) assert.ok(ids('clerk.' + domain).includes('id-servico'));
});

test('A4: id genérico exige contexto Wrangler ou KV; ids explícitos seguem protegidos', () => {
  const uuid = ['12345678', '1234', '1234', '1234', '123456789abc'].join('-');
  const kv = '123456789abcdef0'.repeat(2);
  for (const value of [uuid, kv]) {
    const line = JSON.stringify({ id: value });
    assert.ok(!ids(line).includes('id-servico'));
    assert.ok(scanLine('wrangler.toml', 1, line, policy).some((f) => f.rule === 'id-servico'));
    assert.ok(scanLine('config/wrangler.production.jsonc', 1, line, policy).some((f) => f.rule === 'id-servico'));
    assert.ok(!scanLine('fixtures/dados.json', 1, line, policy).some((f) => f.rule === 'id-servico'));
    assert.ok(ids(JSON.stringify({ kv_namespaces: [{ id: value }] })).includes('id-servico'));
  }
  assert.ok(ids(JSON.stringify({ database_id: uuid })).includes('id-servico'));
  assert.ok(!ids(JSON.stringify({ database_id: '00000000-0000-0000-0000-000000000000' })).includes('id-servico'));
});

test('A4: ACCOUNT_ID prefixado detecta valor e preserva placeholders', () => {
  const value = '123456789abcdef0'.repeat(2);
  for (const key of ['CLOUDFLARE_ACCOUNT_ID', 'APP_CLOUDFLARE_ACCOUNT_ID', 'account_id']) {
    assert.ok(ids(`${key}=${value}`).includes('id-servico'));
    assert.ok(!ids(`${key}=${'0'.repeat(32)}`).includes('id-servico'));
  }
});


test('B1: placeholder Clerk exige domínio reservado e não libera domínio real', () => {
  for (const prefix of ['test', 'your', 'dummy', 'example', 'exemplo']) {
    for (const domain of ['empresa-ficticia.com', 'example.com.empresa-ficticia.com']) {
      assert.ok(ids('clerk.' + prefix + '.' + domain).includes('id-servico'));
    }
    for (const domain of ['fixture.invalid', 'fixture.test', 'example.com', 'example.org', 'example.net']) {
      assert.ok(!ids('clerk.' + prefix + '.' + domain).includes('id-servico'));
      assert.ok(!ids('clerk.' + prefix + '.' + domain + '.').includes('id-servico'));
    }
  }
  assert.ok(ids('clerk.' + 'instancia-ficticia.invalid').includes('id-servico'));
});

test('B2: usuários genéricos Linux não ocultam caminho pessoal desconhecido', () => {
  for (const user of ['runner', 'ubuntu', 'node', 'vscode']) {
    assert.ok(policy.genericUsers.includes(user));
    assert.ok(!ids('/home/' + user + '/projeto').includes('caminho-pessoal'));
  }
  assert.ok(ids('/home/' + 'pessoa-ficticia' + '/projeto').includes('caminho-pessoal'));
});

test('B4: Dependabot sem a lista reprova fechado em qualquer tipo de autor', () => fixture((ctx) => {
  ctx.write('novo.md', 'texto limpo\n'); const head = ctx.commit();
  for (const authorType of ['User', '', 'Bot']) assert.equal(cli({ ...ctx, head, author: 'dependabot[bot]', authorType }).status, 2);
}));

test('B5: commits intermediários do PR são varridos, mesmo que o último apague o conteúdo', () => fixture(({ cwd, git, write, commit, base }) => {
  write('nota.md', `${vault}\n`);
  write('chave.pem', 'fixture\n');
  commit('intermediario');
  git('rm', '-q', 'chave.pem'); write('nota.md', 'texto limpo\n');
  const head = commit('limpa');
  const rules = scan({ cwd, mode: 'pr', base, head, policy }).findings.map((f) => `${f.path}:${f.rule}`);
  assert.ok(rules.includes('nota.md:cofre'), rules.join());
  assert.ok(rules.includes('chave.pem:arquivo-sensivel'), rules.join());
}));

// Exceção estreita de termo (só hash, só arquivo exato, só conteúdo). Termos fictícios.
const hashOf = (t) => createHash('sha256').update(t).digest('hex');
const EXC_FILE = 'pasta/fichas/ficha-ficticia.json';
const excPolicy = (extra = {}) => ({ ...policy, excecoesTermo: [{ sha256: hashOf('cacto ficticio'), arquivo: EXC_FILE, motivo: 'fixture', ...extra }] });
const termRule = (path, line, p, terms = ['Cácto Fictício'], content = true) => scanLine(path, 1, line, p, terms, content).some((f) => f.rule === 'termo-pessoal');

test('exceção de termo: termo exato no arquivo exato passa; mesmo termo em outro arquivo reprova', () => {
  const p = excPolicy();
  assert.equal(termRule(EXC_FILE, 'nome: CACTO-ficticio', p), false);
  for (const other of ['pasta/fichas/outra.json', EXC_FILE + '.bak', 'x/' + EXC_FILE, 'pasta/fichas/', 'PASTA/fichas/ficha-ficticia.json']) assert.equal(termRule(other, 'cacto ficticio', p), true, other);
});

test('exceção de termo: caminho/nome e mensagem de commit continuam reprovando', () => {
  const p = excPolicy();
  assert.equal(termRule(EXC_FILE, EXC_FILE, p, undefined, false), false, 'caminho idêntico sem termo');
  assert.equal(termRule('pasta/cacto-ficticio.json', 'cacto ficticio', p, undefined, false), true);
  assert.equal(termRule('pasta/cacto-ficticio.json', 'x', p), false, 'sem termo no conteúdo');
  assert.equal(termRule('pasta/cacto-ficticio.json', 'cacto ficticio', p), true);
  assert.equal(termRule(`commit/${'a'.repeat(40)}`, 'cacto ficticio', p, undefined, false), true);
  assert.equal(termRule(`commit/${'a'.repeat(40)}`, 'cacto ficticio', p), true, 'caminho de commit nunca é o arquivo exato');
  assert.equal(safePath('pasta/cacto-ficticio.json', ['Cácto Fictício'], p), '[caminho-omitido]');
});

test('exceção de termo: outro termo na mesma linha continua reprovado; hash diferente não exceptua', () => {
  const p = excPolicy();
  const terms = ['Cácto Fictício', 'Agave Inventado'];
  assert.equal(termRule(EXC_FILE, 'cacto ficticio', p, terms), false);
  assert.equal(termRule(EXC_FILE, 'cacto ficticio e agave inventado', p, terms), true);
  assert.equal(termRule(EXC_FILE, 'agave inventado', p, terms), true);
  assert.equal(termRule(EXC_FILE, 'cacto ficticio', excPolicy({ sha256: hashOf('outro termo') }), terms), true);
  assert.equal(termRule(EXC_FILE, 'cacto ficticio', excPolicy({ sha256: hashOf('Cácto Fictício') }), terms), true, 'hash do texto não normalizado não vale');
});

test('exceção de termo: entrada malformada falha fechada', () => {
  const bad = [
    { sha256: 'abc' }, { sha256: hashOf('x').toUpperCase() }, { sha256: 7 }, { arquivo: '../x.json' }, { arquivo: 'a/../x.json' }, { arquivo: '/abs/x.json' },
    { arquivo: 'a/*.json' }, { arquivo: 'pasta/' }, { arquivo: 'a//x.json' }, { arquivo: './x.json' }, { arquivo: 'a\\x.json' }, { arquivo: '' }, { arquivo: 1 }, { motivo: '' }, { motivo: undefined }, { extra: 1 },
  ];
  for (const extra of bad) {
    const p = excPolicy(extra);
    assert.throws(() => termExceptions(p), undefined, JSON.stringify(extra));
    assert.throws(() => scanLine(EXC_FILE, 1, 'cacto ficticio', p, ['Cácto Fictício'], true));
  }
  for (const value of [null, 'x', {}, [null], [[]], ['x']]) assert.throws(() => termExceptions({ ...policy, excecoesTermo: value }));
});

test('exceção de termo: sem excecoesTermo tudo como antes; política do repo é válida e só tem hash', () => {
  const { excecoesTermo, ...semExcecao } = policy;
  assert.deepEqual(termExceptions(semExcecao), []);
  assert.equal(termRule(EXC_FILE, 'cacto ficticio', semExcecao), true);
  assert.deepEqual(termExceptions({ ...policy, excecoesTermo: [] }), []);
  for (const e of termExceptions(policy)) assert.match(e.sha256, /^[a-f0-9]{64}$/);
});

test('exceção de termo: scan em pr e arvore respeita arquivo exato, caminho e commit; loadPolicy valida', () => fixture(({ cwd, git, write, commit, base }) => {
  const p = excPolicy();
  const terms = ['Cácto Fictício'];
  write(EXC_FILE, '{"nome": "cacto ficticio"}\n');
  write('pasta/outro.json', '{"nome": "cacto ficticio"}\n');
  write('pasta/cacto-ficticio.txt', 'limpo\n');
  const head = commit('mensagem limpa');
  for (const mode of ['pr', 'arvore']) {
    const paths = scan({ cwd, mode, base, head, policy: p, terms }).findings.filter((f) => f.rule === 'termo-pessoal').map((f) => f.path).sort();
    assert.deepEqual(paths, ['pasta/cacto-ficticio.txt', 'pasta/outro.json'], mode);
  }
  write('pasta/mais.md', 'limpo\n');
  const msg = commit('nota cacto ficticio');
  assert.ok(scan({ cwd, mode: 'pr', base: head, head: msg, policy: p, terms }).findings.some((f) => f.path.startsWith('commit/') && f.rule === 'termo-pessoal'));
  write('.github/security/vazamento.json', JSON.stringify({ ...policy, excecoesTermo: [{ sha256: 'x', arquivo: '../y', motivo: 'm' }] }));
  const badHead = commit();
  assert.deepEqual(loadPolicy(cwd, base, badHead, true).policy.excecoesTermo, policy.excecoesTermo, 'política do PR não vale; vale a da main');
  git('update-ref', 'refs/remotes/origin/main', badHead);
  assert.throws(() => loadPolicy(cwd, badHead, badHead, true));
}));
