// D519: Node puro. Nunca imprimir conteúdo, termos, erros brutos de Git ou do scanner.
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { readFileSync, writeFileSync, mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { isIP } from 'node:net';
import { crc32, inflateSync } from 'node:zlib';

const RULES = '.github/security/vazamento.json';
const INVENTORY = '.github/security/vazamento-inventario.json';
export const normalize = (s) => s.normalize('NFD').replace(/\p{M}/gu, '').toLowerCase();
const normalizedLine = (s) => normalize(s).trim().replace(/\s+/g, ' ');
export const fingerprint = (rule, path, line) => createHash('sha256').update(`${rule}|${path}|${normalizedLine(line)}`).digest('hex');
const sha = (s) => {
  if (!/^[a-f0-9]{40}$/.test(s ?? '') || /^0+$/.test(s)) throw new Error('SHA inválido');
  return s;
};
function git(cwd, args, binary = false, { input, timeout } = {}) {
  const r = spawnSync('git', ['--no-pager', ...args], {
    cwd, input, timeout, encoding: binary ? undefined : 'utf8', maxBuffer: 256 * 1024 * 1024,
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
const bareClerkTld = /^(?:[a-z]{2}|app|biz|cloud|com|dev|edu|example|gov|info|int|invalid|local|mil|name|net|online|org|site|tech|test|xyz|xn--[a-z\d-]+)$/i;
const genericVaultSegments = new Set(['vault', 'cofre', 'item', 'field', 'campo', 'section', 'seção'].map(normalize));
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
  for (const m of line.matchAll(/(?<![\p{ID_Continue}.$-])(?:([a-z\d](?:[a-z\d-]{0,61}[a-z\d])?)\.|https?:\/\/)clerk\.accounts\.dev\.?(?![\w.-]|\(|\?\.)/giu)) {
    if (!m[1] || !placeholder(m[1], policy)) found.add('id-servico');
  }
  for (const m of line.matchAll(/(?<![\p{ID_Continue}.$-])(https?:\/\/)?clerk\.((?:[a-z\d](?:[a-z\d-]{0,61}[a-z\d])?\.)+[a-z](?:[a-z\d-]{0,61}[a-z\d])?)\.?(?![\w.-]|\(|\?\.)/giu)) {
    const domain = m[2];
    if (!m[1] && !bareClerkTld.test(domain.split('.').at(-1))) continue;
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
  const vault = (path) => {
    const segments = path.replace(/[.,;:!?]+$/, '').split('/').map((segment) => segment.trim());
    if (segments.length >= 2 && segments.every(Boolean) && segments.some((segment) => !genericVaultSegments.has(normalize(segment)) && !placeholder(segment, policy))) found.add('cofre');
  };
  for (const m of line.matchAll(/(["'`])op:\/\/([^"'`\r\n<>()[\]{}?#]*)\1|(?<![\w+.-])op:\/\/([^\s/"'`<>()[\]{}?#]+(?:\/[^\s/"'`<>()[\]{}?#]+)+)/gi)) {
    vault(m[2] ?? m[3]);
  }
  return policy.rules.filter((r) => found.has(r.id) && !(mark && r.marker)).map((r) => r.id);
}
// D523: exclui só o prefixo aprovado em posição de URL, preservando o resto da linha.
function withoutAllowedUrls(line, policy) {
  for (const prefix of policy.urlsPermitidas ?? []) {
    const escaped = Array.from(prefix, (c) => '.?+*|^$()[]{}'.includes(c) || c.charCodeAt(0) === 92 ? String.fromCharCode(92) + c : c).join('');
    const scheme = prefix.startsWith('git@') ? '' : '(https?://)?';
    const boundary = prefix.endsWith('/') ? '' : "(?=(?::[0-9]+)?(?:$|[/?#\\s\"'`<>()\\[\\]{}]))";
    const url = new RegExp('(?<![a-z0-9_./:@-])' + scheme + escaped + boundary, 'gi');
    line = line.replace(url, (match, protocol) => scheme ? (protocol ?? '') : '');
  }
  return normalize(line);
}
export const personalText = (line, policy) => withoutAllowedUrls(line, policy).replace(/[\s._-]+/g, ' ');
const normalizeTerm = (t) => normalize(t).replace(/[\s._-]+/g, ' ');
// Termo com dígito e sem letra (IP, telefone, documento): trocar separador por espaço fabricaria o termo a partir de
// outros números (coordenadas de SVG). Ele casa só literal, com os separadores originais, fora de um número maior;
// ponto e dígitos seguidos de letra (extensão como .7z ou .3gp) não contam como continuação do número.
const numericTerm = (t) => /\p{Nd}/u.test(normalize(t)) && !/\p{L}/u.test(normalize(t));
const numericPatterns = new Map();
const numericPattern = (t) => {
  if (!numericPatterns.has(t)) {
    const body = normalize(t).trim().split(/\s+/).map((part) => part.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')).join('\\s+');
    numericPatterns.set(t, new RegExp('(?<!\\p{Nd}\\.?)' + body + '(?!\\p{Nd})(?!\\.\\p{Nd}+(?!\\p{Nd}|\\.?\\p{L}))', 'u'));
  }
  return numericPatterns.get(t);
};
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
// Data URI de PNG em base64: o base64 de uma imagem tem letras ao acaso que formam termos curtos. Só um PNG completo
// (IHDR primeiro, IDAT com cabeçalho zlib, IEND no fim exato e CRC de todos os chunks), num bloco base64 canônico que termina em aspas,
// parêntese, espaço ou fim de linha, tira o base64 cru da comparação de termos curtos. Termos com 8+ caracteres seguem
// comparados na linha inteira; os textos dos chunks tEXt/zTXt/iTXt e os trechos ASCII legíveis do PNG entram para todos. Base64 de texto acrescenta o texto
// decodificado sem tirar o cru. Qualquer outro caso segue cru, como antes.
const DATA_URI = /data:([\w.+-]*\/[\w.+-]+)?(?:;[\w.+-]+(?:=[\w.+-]*)?)*;base64,([A-Za-z0-9+/]+={0,2})(?=$|["')\s])/gi;
const strictUtf8 = new TextDecoder('utf-8', { fatal: true });
const LONG_TERM = 8;
const PNG_SIGNATURE = Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]);
// Devolve os textos dos chunks tEXt, zTXt e iTXt (inteiros, de qualquer tamanho) ou null se não for um PNG completo.
// `budget` é compartilhado por todos os PNGs da linha: descompressão acima dele deixa o bloco cru.
function completePng(mime, b, budget) {
  if ((mime ?? '').toLowerCase() !== 'image/png' || !b.subarray(0, 8).equals(PNG_SIGNATURE)) return null;
  const texts = [], idat = [];
  const inflate = (z) => {
    if (budget.left <= 0) throw new Error('orçamento esgotado');
    try {
      const out = inflateSync(z, { maxOutputLength: budget.left });
      budget.left -= out.length;
      return out;
    } catch (error) { budget.left = 0; throw error; }
  };
  const field = (data, from) => { const k = data.indexOf(0, from); if (k < from + (from ? 0 : 1) || (from === 0 && k > 79)) throw new Error('chunk de texto inválido'); return k; };
  for (let i = 8; i + 12 <= b.length;) {
    const end = i + 12 + b.readUInt32BE(i);
    const type = b.toString('latin1', i + 4, i + 8);
    if (end > b.length || (crc32(b.subarray(i + 4, end - 4)) >>> 0) !== b.readUInt32BE(end - 4)) return null;
    if (i === 8 && (type !== 'IHDR' || end - i !== 25)) return null;
    const data = b.subarray(i + 8, end - 4);
    try {
      if (type === 'tEXt') {
        const k = field(data, 0);
        if (data.indexOf(0, k + 1) !== -1) return null;
        texts.push(data.toString('latin1', 0, k), data.toString('latin1', k + 1));
      }
      if (type === 'zTXt') {
        const k = field(data, 0);
        if (data[k + 1] !== 0) return null;
        texts.push(data.toString('latin1', 0, k), inflate(data.subarray(k + 2)).toString('latin1'));
      }
      if (type === 'iTXt') {
        const k = field(data, 0);
        if (data.length < k + 3 || data[k + 1] > 1 || data[k + 2] !== 0) return null;
        const lang = field(data, k + 3), tr = field(data, lang + 1), value = data.subarray(tr + 1);
        texts.push(data.toString('latin1', 0, k), data.toString('latin1', k + 3, lang), strictUtf8.decode(data.subarray(lang + 1, tr)),
          strictUtf8.decode(data[k + 1] ? inflate(value) : value));
      }
    } catch { return null; }
    if (type === 'IDAT') idat.push(data.subarray(0, 2));
    if (type === 'IEND') {
      const z = Buffer.concat(idat).subarray(0, 2);
      const zlib = z.length === 2 && (z[0] & 0x0f) === 8 && z[0] >>> 4 <= 7 && (z[1] & 0x20) === 0 && (z[0] * 256 + z[1]) % 31 === 0;
      return zlib && end === b.length ? texts : null;
    }
    i = end;
  }
  return null;
}
// Visão para termos curtos (sem o base64 do PNG) e para termos longos (linha inteira); as duas recebem o texto
// decodificado e os trechos legíveis do PNG.
function termViews(line, onMedia) {
  if (!/;base64,/i.test(line)) return { short: line, long: line };
  const extra = [], budget = { left: 4 * 1024 * 1024 };
  let media = false;
  const short = line.replace(DATA_URI, (whole, mime, block) => {
    const bytes = Buffer.from(block, 'base64');
    if (block.length % 4 === 1 || bytes.toString('base64') !== block.padEnd(Math.ceil(block.length / 4) * 4, '=')) return whole;
    const texts = completePng(mime, bytes, budget);
    if (texts) {
      media = true;
      for (const text of texts) extra.push(text.replace(/\0/g, ' '));
      extra.push((bytes.toString('latin1').match(/[\x20-\x7e]{8,}/g) ?? []).join(' '));
      return whole.slice(0, whole.length - block.length);
    }
    if (!bytes.includes(0)) {
      try { extra.push(strictUtf8.decode(bytes)); } catch { /* não é texto: fica só o cru */ }
    }
    return whole;
  });
  if (media) onMedia();
  const decoded = extra.length ? `\n${extra.join('\n')}` : '';
  return { short: short + decoded, long: line + decoded };
}
// `content` é true só para linhas de conteúdo de arquivo; caminhos e mensagens de commit nunca são exceptuados.
export function scanLine(path, number, line, policy, terms = [], content = false, marker = true, onMedia = () => {}) {
  const ids = publicRules(line, policy, path, marker);
  if (terms.length && termHit(path, line, policy, terms, content, onMedia)) ids.push('termo-pessoal');
  return ids.map((rule) => ({ path, line: number, rule, fingerprint: fingerprint(rule, path, line) }));
}
function termHit(path, line, policy, terms, content, onMedia) {
  const views = termViews(line, onMedia);
  const short = withoutAllowedUrls(views.short, policy);
  const long = views.long === views.short ? short : withoutAllowedUrls(views.long, policy);
  const folded = new Map();
  const personal = (text) => folded.get(text) ?? folded.set(text, text.replace(/[\s._-]+/g, ' ')).get(text);
  const exempt = content ? termExceptions(policy).filter((e) => e.arquivo === path).map((e) => e.sha256) : [];
  const hit = (t) => {
    const literal = normalizeTerm(t).replace(/ /g, '').length >= LONG_TERM ? long : short;
    return numericTerm(t) ? numericPattern(t).test(literal) : personal(literal).includes(normalizeTerm(t));
  };
  return terms.some((t) => !(exempt.length && exempt.includes(termHash(t))) && hit(t));
}
function autoMerge(cwd, [ours, theirs]) {
  const r = spawnSync('git', ['--no-pager', 'merge-tree', '--write-tree', sha(ours), sha(theirs)], { cwd, encoding: 'utf8', maxBuffer: 64 * 1024 * 1024 });
  // 1 = conflito: a árvore sai com marcadores, e a resolução inteira entra na varredura.
  if (![0, 1].includes(r.status)) throw new Error('Operação Git recusada');
  return r.stdout.split('\n')[0];
}
// Toda mudança que não é remoção (inclusive troca de tipo), com o nome canônico do Git; binário pelo conteúdo do blob e diff forçado como texto.
function scanChanges(cwd, from, to, add, findings, limits) {
  const raw = git(cwd, ['diff-tree', '-r', '-z', '--raw', '--no-renames', '--diff-filter=d', from, to]).split('\0');
  for (let i = 0; i + 1 < raw.length; i += 2) {
    const [, mode, oid] = /^:\d+ (\d+) [a-f\d]+ ([a-f\d]+) /.exec(raw[i]) ?? [];
    const path = raw[i + 1];
    if (!oid || !path) throw new Error('Diff inválido');
    if (mode === '160000') { limits.submodule++; continue; }
    if (sensitiveFile(path)) { findings.push({ path, rule: 'arquivo-sensivel', fingerprint: fingerprint('arquivo-sensivel', path, path) }); continue; }
    add(path, 1, path, false, false);
    if (binaryFile(git(cwd, ['cat-file', 'blob', oid], true))) continue;
    const patch = git(cwd, ['-c', 'core.quotePath=false', 'diff', '--no-ext-diff', '--no-textconv', '--text', '--no-renames', '--unified=0', from, to, '--', ':(literal)' + path]);
    let n = 0, hunk = false;
    for (const line of patch.split('\n')) {
      const match = /^@@ -\d+(?:,\d+)? \+(\d+)(?:,\d+)? @@/.exec(line);
      if (match) { n = Number(match[1]); hunk = true; continue; }
      if (hunk && line.startsWith('+')) add(path, n++, line.slice(1), true);
      else if (hunk && line.startsWith(' ')) n++;
    }
  }
}
function scanHistoryTerms(cwd, head, add) {
  const deadline = performance.now() + 5 * 60 * 1000;
  const run = (args, input) => {
    const timeout = Math.ceil(deadline - performance.now());
    if (timeout <= 0) throw new Error('Varredura do histórico não concluída');
    return git(cwd, ['--no-replace-objects', ...args], false, { input, timeout });
  };
  if (run(['rev-parse', '--is-shallow-repository']).trim() !== 'false') throw new Error('Histórico incompleto');
  const commits = run(['rev-list', head, '--']).trim().split('\n').map(sha);
  const emptyTree = sha(run(['hash-object', '-t', 'tree', '--stdin'], '').trim());
  for (let start = 0; start < commits.length; start += 16) {
    const batch = commits.slice(start, start + 16);
    const patch = run([`--attr-source=${emptyTree}`, '-c', 'core.quotePath=false', 'log', '--no-walk=unsorted', '--stdin',
      '--format=%x00%H%x00%B%x00', '--root', '--diff-merges=first-parent', '--no-ext-diff', '--no-textconv', '--no-renames',
      '--no-color', '--no-prefix', '--unified=0', '-p', '--'], batch.join('\n') + '\n');
    const records = patch.split(/^\0([a-f0-9]{40})\0/gm);
    if (records[0] !== '' || records.length !== 1 + 2 * batch.length) throw new Error('Histórico inválido');
    for (let i = 0; i < batch.length; i++) {
      const [commit, body] = records.slice(i * 2 + 1, i * 2 + 3);
      const separator = body.indexOf('\0');
      if (commit !== batch[i] || separator < 0) throw new Error('Histórico inválido');
      const message = body.slice(0, separator), diff = body.slice(separator + 1);
      message.split('\n').forEach((line, n) => add(`commit/${commit}`, n + 1, line, false));
      let path, n = 0, oldLeft = 0, newLeft = 0;
      for (const line of diff.split('\n')) {
        if (oldLeft || newLeft) {
          if (line.startsWith('+') && newLeft) { newLeft--; add(path, n++, line.slice(1), true); }
          else if (line.startsWith('-') && oldLeft) oldLeft--;
          else if (line.startsWith(' ') && oldLeft && newLeft) { oldLeft--; newLeft--; n++; }
          else if (!line.startsWith('\\ ')) throw new Error('Diff histórico inválido');
        } else if (line.startsWith('diff --git ')) path = undefined;
        else if (line.startsWith('+++ ')) {
          const name = line.slice(4).replace(/\t$/, '');
          path = name.startsWith('"') ? JSON.parse(name.replace(/\\(?:[0-7]{3}|.)/g, (escape) => {
            const code = { a: 7, b: 8, v: 11 }[escape[1]] ?? parseInt(escape.slice(1), 8);
            return Number.isNaN(code) ? escape : `\\u${code.toString(16).padStart(4, '0')}`;
          })) : name;
        } else if (line.startsWith('@@ ')) {
          const hunk = /^@@ -\d+(?:,(\d+))? \+(\d+)(?:,(\d+))? @@/.exec(line);
          if (!hunk || !path) throw new Error('Diff histórico inválido');
          oldLeft = Number(hunk[1] ?? 1); n = Number(hunk[2]); newLeft = Number(hunk[3] ?? 1);
        }
      }
      if (oldLeft || newLeft || performance.now() >= deadline) throw new Error('Varredura do histórico não concluída');
    }
  }
}
export function scan({ cwd, mode, base, head, policy, terms = [], inventory = [] }) {
  sha(head);
  if (!['pr', 'arvore'].includes(mode)) throw new Error('Modo inválido');
  if (mode === 'pr') sha(base);
  const findings = [], limits = { binary: 0, sensitive: 0, submodule: 0, base64Media: 0 }, embedded = new Set();
  const add = (path, n, line, content = false, marker = true) => findings.push(...scanLine(path, n, line, policy, terms, content, marker, () => embedded.add(`${path}:${n}`)));
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
    // Merge: compara o merge automático dos pais com o resultado, para pegar só o que a resolução trouxe.
    for (const commit of commits) {
      const parents = git(cwd, ['rev-list', '--parents', '-n', '1', sha(commit)]).trim().split(' ').slice(1);
      if (parents.length > 2) throw new Error('Merge com mais de dois pais não suportado');
      const from = parents.length === 2 ? autoMerge(cwd, parents) : parents[0];
      scanChanges(cwd, sha(from), commit, add, findings, limits);
    }
  } else if (terms.length) {
    scanHistoryTerms(cwd, head, (path, n, line, content) => {
      if (termHit(path, line, policy, terms, content, () => embedded.add(`${path}:${n}`))) {
        findings.push({ path, line: n, rule: 'termo-pessoal', fingerprint: fingerprint('termo-pessoal', path, line) });
      }
    });
  }
  limits.base64Media = embedded.size;
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
    writeFileSync(join(dir, 'config.toml'), git(cwd, ['show', `${sha(trusted)}:.gitleaks.toml`]));
    const baseline = '.github/security/gitleaks-baseline.fingerprints';
    writeFileSync(join(dir, 'baseline'), hasFile(cwd, trusted, baseline) ? git(cwd, ['show', `${trusted}:${baseline}`]) : '');
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
  const range = mode === 'arvore' && !pullRequest ? { head: trusted } : eventRange(cwd, event, process.env.GITHUB_EVENT_NAME, process.env.GITHUB_SHA);
  const { policy, inventory } = loadPolicy(cwd, trusted, range.head, process.env.GITHUB_EVENT_NAME === 'pull_request');
  if (mode === 'arvore' && event.repository.private !== true && inventory.length) throw new Error('Inventário público deve estar vazio');
  const { findings, limits } = scan({ cwd, mode, ...range, policy, terms, inventory: event.repository.private === true ? inventory : [] });
  if (mode === 'arvore' && process.env.GITLEAKS_BIN) findings.push(...historyGitleaks(cwd, process.env.GITLEAKS_BIN, trusted));
  const privateRepo = event.repository.private === true;
  const report = { total: findings.length, rules: summarize(findings).rules };
  if (privateRepo) {
    const safe = findings.map((f) => ({ ...f, path: safePath(f.path, terms, policy) }));
    for (const f of safe) console.log(`${JSON.stringify(f.path)}${f.line ? `:${f.line}` : ''} regra=${f.rule}`);
    console.log(`Limites: binários=${limits.binary}; arquivos sensíveis não lidos=${limits.sensitive}; submódulos=${limits.submodule}; linhas com PNG embutido em base64 (termos curtos só no texto legível)=${limits.base64Media}.`);
    report.paths = [...new Set(safe.map((f) => f.path))].sort();
    report.limits = limits;
  } else {
    for (const [rule, count] of Object.entries(report.rules)) console.log(`regra=${rule} contagem=${count}`);
  }
  if (process.env.VAZAMENTO_REPORT) writeFileSync(process.env.VAZAMENTO_REPORT, JSON.stringify(report, null, 2) + '\n', { mode: 0o600 });
  return findings.length ? 1 : 0;
}
if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main().then((code) => { process.exitCode = code; }).catch(() => {
    console.error('Checagem de vazamento não concluída; conteúdo e erro bruto omitidos.');
    process.exitCode = 2;
  });
}
