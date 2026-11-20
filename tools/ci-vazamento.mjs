// D519: Node puro. Nunca imprimir conteúdo, termos, erros brutos de Git ou do scanner.
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { readFileSync, writeFileSync, mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { isIP } from 'node:net';

const RULES = '.github/security/vazamento.json';
const INVENTORY = '.github/security/vazamento-inventario.json';
export const normalize = (s) => s.normalize('NFD').replace(/\p{M}/gu, '').toLowerCase();
const normalizedLine = (s) => normalize(s).trim().replace(/\s+/g, ' ');
export const fingerprint = (rule, path, line) => createHash('sha256').update(`${rule}|${path}|${normalizedLine(line)}`).digest('hex');
const sha = (s) => {
  if (!/^[a-f0-9]{40}$/.test(s ?? '') || /^0+$/.test(s)) throw new Error('SHA inválido');
  return s;
};
function git(cwd, args, binary = false) {
  const r = spawnSync('git', ['--no-pager', ...args], {
    cwd, encoding: binary ? undefined : 'utf8', maxBuffer: 256 * 1024 * 1024,
  });
  if (r.status !== 0) throw new Error('Operação Git recusada');
  return r.stdout;
}
const hasFile = (cwd, ref, path) => spawnSync('git', ['cat-file', '-e', `${sha(ref)}:${path}`], { cwd, stdio: 'ignore' }).status === 0;
const jsonAt = (cwd, ref, path) => JSON.parse(git(cwd, ['show', `${sha(ref)}:${path}`]));
const sensitiveFile = (path) => /(^|\/)\.env(?:[./]|$)|(?:^|\/)(?:id_rsa|id_ed25519|id_ecdsa|id_dsa)(?:\.|$)|\.(?:pem|key|p12|pfx|keystore)$/i.test(path);
const binaryFile = (data) => data.includes(0);
function placeholder(value, policy) {
  return /^0+(?:-0+)*$/.test(value) || /^<[^>]+>$/.test(value) || policy.placeholderValues.includes(normalize(value));
}
function privateIP(value) {
  const version = isIP(value);
  if (version === 4) {
    const [a, b] = value.split('.').map(Number);
    return a === 10 || (a === 172 && b >= 16 && b <= 31) || (a === 192 && b === 168) ||
      (a === 100 && b >= 64 && b <= 127) || (a === 169 && b === 254);
  }
  if (version === 6) {
    const first = parseInt(value.split(':')[0] || '0', 16);
    return (first & 0xfe00) === 0xfc00 || (first & 0xffc0) === 0xfe80;
  }
  return false;
}
export function publicRules(line, policy, path = '', marker = true) {
  const found = new Set();
  const mark = marker && /\bvazamento-ok:\s*\S+/i.test(line);
  for (const m of line.matchAll(/(?:\/(?:Users|home)\/|\b[A-Za-z]:\\+Users\\+)(<[^>]+>|\$\{?USER\}?|[^\s/\\"'`<>:,;]+)/gi)) {
    if (!policy.genericUsers.some((v) => normalize(v) === normalize(m[1]))) found.add('caminho-pessoal');
  }
  for (const m of line.matchAll(/(?<![\w.])(?:\d{1,3}\.){3}\d{1,3}(?!\w|\.\d)/g)) {
    if (privateIP(m[0])) found.add('ip-privado');
  }
  for (const m of line.matchAll(/(?<![\w:])(?:[a-f\d]{0,4}:){2,}[a-f\d:.]*(?:%[\w-]+)?/gi)) {
    if (privateIP(m[0].split('%')[0].replace(/\.+$/, ''))) found.add('ip-privado');
  }
  if (/\b(?:[a-z\d-]+\.)+ts\.net\b/i.test(line) || /^\s*Host\s+(?![<*?!])\S+/i.test(line) ||
      /(?:https?:\/\/|ssh:\/\/|\b(?:host(?:name)?|server|address)["']?\s*[=:]\s*["']?|\bssh\s+(?:[\w.-]+@)?)[\w.-]+\.local\b/i.test(line)) found.add('host-interno');
  const service = (value) => { if (!placeholder(value, policy)) found.add('id-servico'); };
  for (const m of line.matchAll(/\b(?:[ps]k_(?:live|test)_|whsec_)([\w-]+|<[^>]+>)/g)) service(m[1]);
  for (const m of line.matchAll(/\b(?:ins|user)_([A-Za-z\d]{16,}|<[^>]+>)/g)) service(m[1]);
  for (const m of line.matchAll(/\bclerk\.([\w.-]+)/gi)) {
    const domain = m[1].replace(/\.$/, '');
    const reserved = /\.(?:invalid|test)$|(?:^|\.)example\.(?:com|org|net)$/i.test(domain);
    if (!(reserved && placeholder(domain.split('.')[0], policy))) found.add('id-servico');
  }
  for (const m of line.matchAll(/\b([\w-]+)\.workers\.dev\b/gi)) service(m[1]);
  for (const m of line.matchAll(/\b(?:(?:[a-z\d]+[_-])*account[_-]id|(?:kv[_-]?(?:namespace[_-]?)?id)|(?:binding|namespace)[_-]?id)["']?\s*[:=]\s*["']?([a-f\d]{32}|<[^>]+>)/gi)) service(m[1]);
  for (const m of line.matchAll(/\b(?:database_id|d1[_-]?(?:database[_-]?)?id)["']?\s*[:=]\s*["']?([a-f\d]{8}(?:-[a-f\d]{4}){3}-[a-f\d]{12}|[a-f\d]{32}|<[^>]+>)/gi)) service(m[1]);
  // Generic `id` is a service identifier only with explicit Wrangler/KV context.
  if (/(?:^|\/)wrangler(?:\.[^/]+)?\.(?:toml|jsonc?)$/i.test(path) || /\bkv_namespaces\b/i.test(line)) {
    for (const m of line.matchAll(/\bid["']?\s*[:=]\s*["']?([a-f\d]{8}(?:-[a-f\d]{4}){3}-[a-f\d]{12}|[a-f\d]{32}|<[^>]+>)/gi)) service(m[1]);
  }
  if (/op:\/\//i.test(line)) found.add('cofre');
  return policy.rules.filter((r) => found.has(r.id) && !(mark && r.marker)).map((r) => r.id);
}
// D523: exclui só o prefixo aprovado em posição de URL, preservando o resto da linha.
export function personalText(line, policy) {
  for (const prefix of policy.urlsPermitidas ?? []) {
    const escaped = Array.from(prefix, (c) => '.?+*|^$()[]{}'.includes(c) || c.charCodeAt(0) === 92 ? String.fromCharCode(92) + c : c).join('');
    const scheme = prefix.startsWith('git@') ? '' : '(https?://)?';
    const boundary = prefix.endsWith('/') ? '' : "(?=(?::[0-9]+)?(?:$|[/?#\\s\"'`<>()\\[\\]{}]))";
    const url = new RegExp('(?<![a-z0-9_./:@-])' + scheme + escaped + boundary, 'gi');
    line = line.replace(url, (match, protocol) => scheme ? (protocol ?? '') : '');
  }
  return normalize(line).replace(/[\s._-]+/g, ' ');
}
const normalizeTerm = (t) => normalize(t).replace(/[\s._-]+/g, ' ');
const termHash = (t) => createHash('sha256').update(normalizeTerm(t)).digest('hex');
// Exceção estreita de termo: só o hash do termo normalizado, num caminho exato, para o conteúdo (nunca nome/commit).
// Formato inválido falha fechado (lança).
export function termExceptions(policy) {
  const list = policy.excecoesTermo;
  if (list === undefined) return [];
  if (!Array.isArray(list)) throw new Error('excecoesTermo inválido');
  return list.map((e) => {
    const ok = e && typeof e === 'object' && !Array.isArray(e) && Object.keys(e).every((k) => ['sha256', 'arquivo', 'motivo'].includes(k)) &&
      typeof e.sha256 === 'string' && /^[a-f0-9]{64}$/.test(e.sha256) &&
      typeof e.motivo === 'string' && e.motivo.trim() &&
      typeof e.arquivo === 'string' && e.arquivo && !/[\x00-\x1f\x7f\\*?[\]{}]/.test(e.arquivo) &&
      !e.arquivo.startsWith('/') && !e.arquivo.endsWith('/') && e.arquivo.split('/').every((seg) => seg && seg !== '.' && seg !== '..');
    if (!ok) throw new Error('excecoesTermo inválido');
    return e;
  });
}
// `content` é true só para linhas de conteúdo de arquivo; caminhos e mensagens de commit nunca são exceptuados.
export function scanLine(path, number, line, policy, terms = [], content = false, marker = true) {
  const ids = publicRules(line, policy, path, marker);
  const personal = terms.length ? personalText(line, policy) : '';
  const exempt = content && terms.length ? new Set(termExceptions(policy).filter((e) => e.arquivo === path).map((e) => e.sha256)) : new Set();
  if (terms.some((t) => !exempt.has(termHash(t)) && personal.includes(normalizeTerm(t)))) ids.push('termo-pessoal');
  return ids.map((rule) => ({ path, line: number, rule, fingerprint: fingerprint(rule, path, line) }));
}
export function scan({ cwd, mode, base, head, policy, terms = [], inventory = [] }) {
  sha(head);
  if (!['pr', 'arvore'].includes(mode)) throw new Error('Modo inválido');
  if (mode === 'pr') sha(base);
  const findings = [], limits = { binary: 0, sensitive: 0, submodule: 0 };
  const add = (path, n, line, content = false, marker = true) => findings.push(...scanLine(path, n, line, policy, terms, content, marker));
  // Listing object IDs avoids reading working-tree symlinks, filters and untracked secrets.
  const entries = git(cwd, ['ls-tree', '-rz', head]).split('\0').filter(Boolean);
  const changed = mode === 'pr' ? new Set(git(cwd, ['diff', '--name-only', '--no-renames', '-z', base, head]).split('\0')) : null;
  for (const entry of entries) {
    const [, , type, oid, path] = /^(\d+) (\w+) ([a-f\d]+)\t([\s\S]+)$/.exec(entry) ?? [];
    if (!path) throw new Error('Árvore inválida');
    if (changed && !changed.has(path)) continue;
    if (type === 'commit') { limits.submodule++; continue; }
    if (sensitiveFile(path)) {
      limits.sensitive++;
      findings.push({ path, rule: 'arquivo-sensivel', fingerprint: fingerprint('arquivo-sensivel', path, path) });
      continue;
    }
    // A path itself may contain an identifier. Never allow it to escape in reports; the marker never exempts a path.
    add(path, 1, path, false, false);
    const data = git(cwd, ['cat-file', 'blob', oid], true);
    if (binaryFile(data)) { limits.binary++; continue; }
    if (mode === 'arvore') {
      data.toString('utf8').split('\n').forEach((line, i) => add(path, i + 1, line, true));
    } else {
      const patch = git(cwd, ['-c', 'core.quotePath=false', 'diff', '--no-ext-diff', '--no-textconv', '--text', '--no-renames', '--unified=0', base, head, '--', `:(literal)${path}`]);
      let n = 0, hunk = false;
      for (const line of patch.split('\n')) {
        const match = /^@@ -\d+(?:,\d+)? \+(\d+)(?:,\d+)? @@/.exec(line);
        if (match) { n = Number(match[1]); hunk = true; continue; }
        if (hunk && line.startsWith('+')) add(path, n++, line.slice(1), true);
        else if (hunk && line.startsWith(' ')) n++;
      }
    }
  }
  if (mode === 'pr') {
    const commits = git(cwd, ['rev-list', `${base}..${head}`]).trim().split('\n').filter(Boolean);
    for (const commit of commits) {
      git(cwd, ['show', '-s', '--format=%B', sha(commit)]).split('\n').forEach((line, i) => add(`commit/${commit}`, i + 1, line));
    }
    // Commits intermediários ficam no histórico depois de um merge normal: cada um é varrido também.
    const own = git(cwd, ['rev-list', '--no-merges', `${base}..${head}`]).trim().split('\n').filter(Boolean);
    for (const commit of own) {
      for (const path of git(cwd, ['diff-tree', '--no-commit-id', '-r', '-z', '--name-only', '--no-renames', '--diff-filter=AM', sha(commit)]).split('\0').filter(Boolean)) {
        if (sensitiveFile(path)) findings.push({ path, rule: 'arquivo-sensivel', fingerprint: fingerprint('arquivo-sensivel', path, path) });
        else add(path, 1, path, false, false);
      }
      const patch = git(cwd, ['-c', 'core.quotePath=false', 'show', '--format=', '--no-ext-diff', '--no-textconv', '--no-renames', '--unified=0', sha(commit)]);
      let path = null, n = 0, hunk = false;
      for (const line of patch.split('\n')) {
        if (line.startsWith('diff --git ')) { path = null; hunk = false; continue; }
        if (!hunk && line.startsWith('+++ ')) { path = line === '+++ /dev/null' ? null : line.slice(4).replace(/^"?b\//, '').replace(/"$/, ''); continue; }
        const match = /^@@ -\d+(?:,\d+)? \+(\d+)(?:,\d+)? @@/.exec(line);
        if (match) { n = Number(match[1]); hunk = true; continue; }
        if (hunk && path && !sensitiveFile(path) && line.startsWith('+')) add(path, n++, line.slice(1), true);
      }
    }
  }
  const unique = [...new Map(findings.map((f) => [`${f.path}:${f.line}:${f.rule}:${f.fingerprint}`, f])).values()];
  return { findings: unique.filter((f) => mode !== 'arvore' || !inventory.includes(f.fingerprint)), limits };
}
export function summarize(findings) {
  const rules = {}, folders = {};
  for (const f of findings) {
    rules[f.rule] = (rules[f.rule] || 0) + 1;
    const folder = f.path.includes('/') ? f.path.split('/')[0] : '(raiz)';
    folders[folder] = (folders[folder] || 0) + 1;
  }
  return { rules, folders };
}
export function safePath(path, terms = [], policy) {
  // Em nomes de arquivo o marcador não dispensa regra: o caminho vai para log e relatório.
  return scanLine('', 0, path, policy, terms, false, false).length || /[\x00-\x1f\x7f]/.test(path) ? '[caminho-omitido]' : path;
}
export function maskTerms(raw, emit) {
  const terms = raw.split(/\r?\n/).map((s) => s.trim()).filter(Boolean);
  // Each line is a command; escape the Actions command protocol before emitting it.
  for (const term of terms) {
    for (const value of new Set([term, normalize(term)])) emit(`::add-mask::${value.replace(/%/g, '%25').replace(/\r/g, '%0D').replace(/\n/g, '%0A')}`);
  }
  return terms;
}
export function loadPolicy(cwd, trusted, head, bootstrap) {
  sha(trusted); sha(head);
  if (trusted !== git(cwd, ['rev-parse', 'refs/remotes/origin/main']).trim()) throw new Error('Main confiável divergente');
  let source = trusted;
  if (!hasFile(cwd, trusted, 'tools/ci-vazamento.mjs')) {
    if (!bootstrap || hasFile(cwd, trusted, RULES) || hasFile(cwd, trusted, INVENTORY)) throw new Error('Bootstrap recusado');
    source = head;
  }
  const policy = jsonAt(cwd, source, RULES);
  termExceptions(policy);
  return { policy, inventory: jsonAt(cwd, source, INVENTORY).fingerprints };
}
function eventRange(cwd, event, name, githubSha) {
  if (name === 'pull_request') return { base: git(cwd, ['merge-base', sha(event.pull_request.base.sha), sha(event.pull_request.head.sha)]).trim(), head: sha(event.pull_request.head.sha) };
  if (name === 'push') return { base: sha(event.before), head: sha(event.after) };
  if (name === 'merge_group') return { base: sha(event.merge_group.base_sha), head: sha(event.merge_group.head_sha) };
  const head = sha(githubSha);
  return { base: git(cwd, ['rev-parse', `${head}^`]).trim(), head };
}
function historyGitleaks(cwd, bin, trusted) {
  const dir = mkdtempSync(join(tmpdir(), 'vazamento-gitleaks-'));
  try {
    for (const [path, file] of [['.gitleaks.toml', 'config.toml'], ['.github/security/gitleaks-baseline.fingerprints', 'baseline']]) {
      writeFileSync(join(dir, file), git(cwd, ['show', `${sha(trusted)}:${path}`]));
    }
    const source = join(dir, 'repository.git');
    const clone = spawnSync('git', ['clone', '--bare', '--shared', '--no-hardlinks', '--', cwd, source], { encoding: 'utf8' });
    if (clone.status !== 0) throw new Error('Isolamento do histórico recusado');
    const report = join(dir, 'report.json');
    const result = spawnSync(bin, ['git', '--redact=100', '--no-banner', '--ignore-gitleaks-allow', '--config', join(dir, 'config.toml'), '--gitleaks-ignore-path', join(dir, 'baseline'), '--log-opts', '--all', '--report-format', 'json', '--report-path', report, source], { encoding: 'utf8', maxBuffer: 256 * 1024 * 1024 });
    if (![0, 1].includes(result.status)) throw new Error('Varredura do histórico não concluída');
    const rows = JSON.parse(readFileSync(report, 'utf8'));
    if (result.status === 1 && !rows.length) throw new Error('Relatório incompleto');
    return rows.map((r) => ({ path: r.File, line: r.StartLine, rule: 'gitleaks' }));
  } finally { rmSync(dir, { recursive: true, force: true }); }
}
export async function notify(report, { sourcePrivate, repository, target, token }, request = fetch) {
  if (!report.total) return;
  // A public source can only send to an explicitly configured, verified private destination.
  if (!sourcePrivate && (!target || !token)) throw new Error('Destino privado obrigatório');
  const destination = target || repository;
  if (!/^[\w.-]+\/[\w.-]+$/.test(destination || '') || !token) throw new Error('Destino indisponível');
  const headers = { Authorization: `Bearer ${token}`, Accept: 'application/vnd.github+json', 'X-GitHub-Api-Version': '2022-11-28' };
  const repo = await request(`https://api.github.com/repos/${destination}`, { headers, redirect: 'error' });
  if (!repo.ok || (await repo.json()).private !== true) throw new Error('Issue recusada: destino não privado');
  const counts = `Contagens por regra:\n${Object.entries(report.rules).map(([rule, n]) => `${rule}: ${n}`).join('\n')}`;
  const body = sourcePrivate && Array.isArray(report.paths) ? `${counts}\n\nCaminhos:\n${report.paths.map((p) => JSON.stringify(p)).join('\n')}` : counts;
  const response = await request(`https://api.github.com/repos/${destination}/issues`, { method: 'POST', headers, redirect: 'error', body: JSON.stringify({ title: 'D519: ocorrências na varredura semanal', body }) });
  if (!response.ok) throw new Error('Issue não criada');
}
export async function main(args = process.argv.slice(2)) {
  // Only named variables, and only in the CLI. Importing this module never reads secrets.
  const mode = args[0] || 'pr';
  if (mode === 'notificar') {
    await notify(JSON.parse(readFileSync(args[1], 'utf8')), {
      sourcePrivate: process.env.VAZAMENTO_SOURCE_PRIVATE === 'true', repository: process.env.GITHUB_REPOSITORY,
      target: process.env.VAZAMENTO_ISSUE_REPO, token: process.env.VAZAMENTO_ISSUE_TOKEN || (process.env.VAZAMENTO_SOURCE_PRIVATE === 'true' ? process.env.GITHUB_TOKEN : undefined),
    });
    return 0;
  }
  const terms = maskTerms(process.env.VAZAMENTO_TERMOS || '', (s) => console.log(s));
  const cwd = process.cwd();
  const event = JSON.parse(readFileSync(process.env.GITHUB_EVENT_PATH, 'utf8'));
  const pullRequest = process.env.GITHUB_EVENT_NAME === 'pull_request';
  const fork = pullRequest && event.pull_request.head.repo.fork === true &&
    event.pull_request.head.repo.full_name !== event.repository.full_name;
  const trusted = sha(process.env.VAZAMENTO_TRUSTED_MAIN_SHA);
  // Instalação inicial: a main ainda não tem o executor, então ele veio do PR e não recebe o secret.
  const inicial = pullRequest && !hasFile(cwd, trusted, 'tools/ci-vazamento.mjs');
  // Sem exceção por autor: o Dependabot recebe a lista pelo secret de Dependabot de mesmo nome.
  if (!terms.length && !fork && !inicial) throw new Error('Secret obrigatório ausente');
  if (!terms.length) console.log(`termos pessoais: não verificados (${fork ? 'fork' : 'instalação inicial'})`);
  const range = eventRange(cwd, event, process.env.GITHUB_EVENT_NAME, process.env.GITHUB_SHA);
  const { policy, inventory } = loadPolicy(cwd, trusted, range.head, process.env.GITHUB_EVENT_NAME === 'pull_request');
  if (mode === 'arvore' && event.repository.private !== true && inventory.length) throw new Error('Inventário público deve estar vazio');
  const { findings, limits } = scan({ cwd, mode, ...range, policy, terms, inventory: event.repository.private === true ? inventory : [] });
  if (mode === 'arvore' && process.env.GITLEAKS_BIN) findings.push(...historyGitleaks(cwd, process.env.GITLEAKS_BIN, trusted));
  const safe = findings.map((f) => ({ ...f, path: safePath(f.path, terms, policy) }));
  const privateRepo = event.repository.private === true;
  const report = { total: safe.length, rules: summarize(safe).rules };
  if (privateRepo) {
    for (const f of safe) console.log(`${JSON.stringify(f.path)}${f.line ? `:${f.line}` : ''} regra=${f.rule}`);
    console.log(`Limites: binários=${limits.binary}; arquivos sensíveis não lidos=${limits.sensitive}; submódulos=${limits.submodule}.`);
    report.paths = [...new Set(safe.map((f) => f.path))].sort();
    report.limits = limits;
  } else {
    for (const [rule, count] of Object.entries(report.rules)) console.log(`regra=${rule} contagem=${count}`);
  }
  if (process.env.VAZAMENTO_REPORT) writeFileSync(process.env.VAZAMENTO_REPORT, JSON.stringify(report, null, 2) + '\n', { mode: 0o600 });
  return safe.length ? 1 : 0;
}
if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main().then((code) => { process.exitCode = code; }).catch(() => {
    console.error('Checagem de vazamento não concluída; conteúdo e erro bruto omitidos.');
    process.exitCode = 2;
  });
}
