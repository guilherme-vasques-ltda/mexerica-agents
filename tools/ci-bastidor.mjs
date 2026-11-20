// Trava contra bastidor em texto que vai ao GitHub (título e descrição de PR, branch, commits e comentários).
// Node puro, sem dependências. Nunca imprime o trecho achado: só categoria, regra, campo e linha.
// Modos: pr (lê o PR pela API), comentario (lê o evento de comentário) e local (antes de publicar).
// Guia: docs/guides/bastidor.md.
import { spawnSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const REGRAS_PADRAO = resolve(dirname(fileURLToPath(import.meta.url)), '../.github/security/bastidor.json');
const VARIAVEL = 'BASTIDOR_NOMES_INTERNOS';
const ACOES_PERMITIDAS = new Set(['admin', 'maintain', 'write']);

export const normalizar = (texto) => texto
  .normalize('NFKD')
  .replace(/\p{M}/gu, '')
  .replace(/[\u00ad\u200b-\u200f\u2060\ufeff]/g, '')
  .replace(/&nbsp;/gi, ' ')
  .toLowerCase();

// Marcação (tags, ênfase, colchetes) não pode partir uma frase: cada linha é testada crua e sem marcação.
const semMarcacao = (linha) => linha
  .replace(/<(?!!--)[^>\n]*>/g, ' ')
  .replace(/[*`[\]~]/g, '')
  .replace(/(?<![a-z0-9])_|_(?![a-z0-9])/g, '');

const escapar = (texto) => texto.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');

function expandir(padrao, definicoes) {
  let atual = padrao;
  for (let i = 0; i < 5 && /\{\{\w+\}\}/.test(atual); i++) {
    atual = atual.replace(/\{\{(\w+)\}\}/g, (_, nome) => {
      if (!(nome in definicoes)) throw new Error('Definição ausente nas regras');
      return definicoes[nome];
    });
  }
  if (/\{\{/.test(atual)) throw new Error('Definição circular nas regras');
  return atual;
}

const listaDe = (valor, formato, limite = 50) => {
  if (!Array.isArray(valor) || valor.length > limite || !valor.every((v) => typeof v === 'string' && formato.test(v))) {
    throw new Error(`${VARIAVEL} inválida`);
  }
  return valor;
};

// Formato da variável: { prefixosTarefa: ["AB"], plataformas: ["nome"], termosInternos: ["termo"], automacao: ["conta[bot]"] }.
export function lerInternos(bruto) {
  if (bruto === undefined || bruto === null || !String(bruto).trim()) return null;
  let dados;
  try { dados = JSON.parse(bruto); } catch { throw new Error(`${VARIAVEL} inválida`); }
  if (!dados || typeof dados !== 'object' || Array.isArray(dados)) throw new Error(`${VARIAVEL} inválida`);
  if (Object.keys(dados).some((k) => !['prefixosTarefa', 'plataformas', 'termosInternos', 'automacao'].includes(k))) throw new Error(`${VARIAVEL} inválida`);
  return {
    prefixosTarefa: listaDe(dados.prefixosTarefa ?? [], /^[A-Za-z]{2,8}$/),
    plataformas: listaDe(dados.plataformas ?? [], /^[A-Za-z0-9][A-Za-z0-9. -]{1,39}$/),
    termosInternos: listaDe(dados.termosInternos ?? [], /^[A-Za-z0-9][A-Za-z0-9. -]{1,39}$/),
    automacao: listaDe(dados.automacao ?? [], /^[A-Za-z0-9-]{1,39}(?:\[bot\])?$/),
  };
}

const alternativas = (lista) => lista.map((v) => escapar(normalizar(v).trim()).replace(/\s+/g, '\\s+')).join('|');

export function compilarRegras(config, internos = null) {
  if (config?.versao !== 1 || !Array.isArray(config.regras)) throw new Error('Regras inválidas');
  const regras = config.regras.map((r) => ({
    id: r.id,
    categoria: r.categoria,
    campos: r.campos ?? null,
    re: new RegExp(expandir(r.padrao, config.definicoes ?? {}), 'u'),
  }));
  if (internos?.prefixosTarefa.length) {
    regras.push({ id: 'tarefa-codigo', categoria: 'tarefa', campos: null, re: new RegExp(`(?<![a-z0-9])(?:${alternativas(internos.prefixosTarefa)})[-_]\\d{1,6}(?![a-z0-9])`, 'u') });
  }
  if (internos?.plataformas.length) {
    regras.push({ id: 'plataforma-nome', categoria: 'plataforma', campos: null, re: new RegExp(`(?<![a-z0-9])(?:${alternativas(internos.plataformas)})(?![a-z0-9])`, 'u') });
  }
  if (internos?.termosInternos.length) {
    regras.push({ id: 'termo-interno', categoria: 'coordenacao', campos: null, re: new RegExp(`(?<![a-z0-9])(?:${alternativas(internos.termosInternos)})(?![a-z0-9])`, 'u') });
  }
  return { regras, orientacao: config.orientacao ?? {}, completo: Boolean(internos) };
}

export function carregarRegras(arquivo = REGRAS_PADRAO, bruto = process.env[VARIAVEL]) {
  return compilarRegras(JSON.parse(readFileSync(arquivo, 'utf8')), lerInternos(bruto));
}

const rotuloCommit = (sha, indice) => `commit ${/^[0-9a-f]{7,40}$/i.test(sha ?? '') ? sha.slice(0, 7).toLowerCase() : indice + 1}`;

// entrada: { titulo, descricao, branch, commits: [{ sha, mensagem }], comentario }. Devolve só posições, nunca o trecho.
export function verificar(entrada, { regras }) {
  const campos = [];
  if (entrada.titulo != null) campos.push(['titulo', entrada.titulo]);
  if (entrada.descricao != null) campos.push(['descricao', entrada.descricao]);
  if (entrada.branch != null) campos.push(['branch', entrada.branch]);
  (entrada.commits ?? []).forEach((c, i) => campos.push([rotuloCommit(c.sha, i), c.mensagem ?? '']));
  if (entrada.comentario != null) campos.push(['comentario', entrada.comentario]);
  const achados = [];
  for (const [campo, texto] of campos) {
    const tipo = campo.split(' ')[0];
    const vistos = new Set();
    normalizar(String(texto)).split(/\r\n|\r|\n/).forEach((linha, i) => {
      const variantes = [linha, semMarcacao(linha)];
      for (const regra of regras) {
        if (regra.campos && !regra.campos.includes(tipo)) continue;
        const chave = `${regra.id}|${i}`;
        if (vistos.has(chave) || !variantes.some((v) => regra.re.test(v))) continue;
        vistos.add(chave);
        achados.push({ categoria: regra.categoria, regra: regra.id, campo, linha: i + 1 });
      }
    });
  }
  return achados;
}

export function formatar(achados, { orientacao = {}, completo = true } = {}) {
  const linhas = [];
  if (!completo) {
    linhas.push(`::warning title=Bastidor::A variável ${VARIAVEL} não está definida: códigos de tarefa, nome da plataforma e termos internos não foram verificados.`);
  }
  for (const a of achados) {
    linhas.push(`::error title=Bastidor::[${a.categoria}] campo ${a.campo}, linha ${a.linha} (regra ${a.regra})`);
  }
  const dicas = [...new Set(achados.map((a) => orientacao[a.categoria]).filter(Boolean))];
  for (const dica of dicas) linhas.push(dica);
  if (achados.length) linhas.push('Reescreva só com o que mudou, por quê, como foi testado e o que ficou sem provar. Guia: docs/guides/bastidor.md.');
  return linhas.join('\n');
}

async function api(env, caminho, { metodo = 'GET', corpo } = {}, buscar = fetch) {
  const resposta = await buscar(`${env.GITHUB_API_URL ?? 'https://api.github.com'}${caminho}`, {
    method: metodo,
    headers: {
      Authorization: `Bearer ${env.GH_TOKEN ?? env.GITHUB_TOKEN}`,
      Accept: 'application/vnd.github+json',
      'X-GitHub-Api-Version': '2022-11-28',
      ...(corpo ? { 'Content-Type': 'application/json' } : {}),
    },
    body: corpo ? JSON.stringify(corpo) : undefined,
  });
  if (!resposta.ok) throw new Error(`API recusou ${metodo} (${resposta.status})`);
  return resposta.status === 204 ? null : resposta.json();
}

const repositorio = (env) => {
  if (!/^(?!\.{1,2}\/)[\w.-]+\/(?!\.{1,2}$)[\w.-]+$/.test(env.GITHUB_REPOSITORY ?? '')) throw new Error('GITHUB_REPOSITORY inválido');
  return env.GITHUB_REPOSITORY;
};

export async function lerPr(env, buscar = fetch) {
  const repo = repositorio(env);
  const numero = Number(env.PR_NUMBER);
  if (!Number.isInteger(numero) || numero < 1) throw new Error('PR_NUMBER inválido');
  const pr = await api(env, `/repos/${repo}/pulls/${numero}`, {}, buscar);
  const commits = [];
  for (let pagina = 1; pagina <= 3; pagina++) {
    const lote = await api(env, `/repos/${repo}/pulls/${numero}/commits?per_page=100&page=${pagina}`, {}, buscar);
    commits.push(...lote.map((c) => ({ sha: c.sha, mensagem: c.commit?.message ?? '' })));
    if (lote.length < 100) break;
  }
  if (commits.length < (pr.commits ?? 0)) throw new Error('PR com mais commits do que a API lista: confira localmente');
  return { titulo: pr.title ?? '', descricao: pr.body ?? '', branch: pr.head?.ref ?? '', commits };
}

// Comentário em issue ou PR, comentário de revisão em linha e corpo de revisão.
export function extrairComentario(nomeEvento, evento) {
  if (evento.action === 'deleted') return null;
  const de = (tipo, fonte, numero) => ({ tipo, corpo: fonte?.body, id: fonte?.id, nodeId: fonte?.node_id, usuario: fonte?.user, numero });
  if (nomeEvento === 'issue_comment') return de('issue', evento.comment, evento.issue?.number);
  if (nomeEvento === 'pull_request_review_comment') return de('comentario-de-revisao', evento.comment, evento.pull_request?.number);
  if (nomeEvento === 'pull_request_review') return de('revisao', evento.review, evento.pull_request?.number);
  return null;
}

const ehBot = (usuario) => usuario?.type === 'Bot' || /\[bot\]$/i.test(usuario?.login ?? '');

// Só a automação do projeto (lista da variável) e quem tem escrita no repositório têm o comentário removido.
export async function autorElegivel(env, usuario, internos, buscar = fetch) {
  const login = usuario?.login;
  if (!login || !/^[A-Za-z0-9-]{1,39}(?:\[bot\])?$/.test(login)) return false;
  if (ehBot(usuario)) return (internos?.automacao ?? []).some((v) => v.toLowerCase() === login.toLowerCase());
  const permissao = await api(env, `/repos/${repositorio(env)}/collaborators/${login}/permission`, {}, buscar);
  return ACOES_PERMITIDAS.has(permissao.permission);
}

export async function tratarComentario(env, nomeEvento, evento, regras, internos, buscar = fetch) {
  const dados = extrairComentario(nomeEvento, evento);
  if (!dados || !dados.corpo || !dados.corpo.trim()) return { resultado: 'ignorado', achados: [] };
  const achados = verificar({ comentario: dados.corpo }, regras);
  if (!achados.length) return { resultado: 'limpo', achados };
  if (!(await autorElegivel(env, dados.usuario, internos, buscar))) return { resultado: 'fora-do-escopo', achados };
  const repo = repositorio(env);
  let acao;
  if (dados.tipo === 'issue') {
    await api(env, `/repos/${repo}/issues/comments/${dados.id}`, { metodo: 'DELETE' }, buscar);
    acao = 'removido';
  } else if (dados.tipo === 'comentario-de-revisao') {
    await api(env, `/repos/${repo}/pulls/comments/${dados.id}`, { metodo: 'DELETE' }, buscar);
    acao = 'removido';
  } else {
    // O corpo de uma revisão enviada não pode ser apagado pela API: fica oculto.
    const consulta = 'mutation($id: ID!) { minimizeComment(input: {subjectId: $id, classifier: OFF_TOPIC}) { minimizedComment { isMinimized } } }';
    const graphql = env.GITHUB_GRAPHQL_URL ?? `${env.GITHUB_API_URL ?? 'https://api.github.com'}/graphql`;
    const resposta = await buscar(graphql, {
      method: 'POST',
      headers: { Authorization: `Bearer ${env.GH_TOKEN ?? env.GITHUB_TOKEN}`, 'Content-Type': 'application/json' },
      body: JSON.stringify({ query: consulta, variables: { id: dados.nodeId } }),
    });
    const corpo = resposta.ok ? await resposta.json() : { errors: [1] };
    if (corpo.errors) throw new Error('API recusou ocultar a revisão');
    acao = 'ocultado';
  }
  const categorias = [...new Set(achados.map((a) => a.categoria))].join(', ');
  const aviso = `Um comentário de @${dados.usuario.login} foi ${acao} por conter texto interno (categoria: ${categorias}). `
    + 'Reescreva só com o que mudou, por quê, como foi testado e o que ficou sem provar. Guia: docs/guides/bastidor.md.';
  await api(env, `/repos/${repo}/issues/${dados.numero}/comments`, { metodo: 'POST', corpo: { body: aviso } }, buscar);
  return { resultado: acao, achados };
}

function lerArgumentos(argv) {
  const opcoes = {};
  for (let i = 0; i < argv.length; i++) {
    if (!argv[i].startsWith('--') || i + 1 >= argv.length) throw new Error('Argumento inválido');
    opcoes[argv[i].slice(2)] = argv[++i];
  }
  return opcoes;
}

const git = (args) => {
  const r = spawnSync('git', ['--no-pager', ...args], { encoding: 'utf8', maxBuffer: 64 * 1024 * 1024 });
  if (r.status !== 0) throw new Error('Operação Git recusada');
  return r.stdout;
};

export function lerLocal(opcoes) {
  const base = opcoes.base ?? 'origin/main';
  const commits = git(['log', '--format=%H%x1f%B%x1e', `${base}..HEAD`]).split('\x1e')
    .map((bloco) => bloco.replace(/^\n/, ''))
    .filter(Boolean)
    .map((bloco) => { const [sha, ...resto] = bloco.split('\x1f'); return { sha, mensagem: resto.join('\x1f') }; });
  return {
    titulo: opcoes.titulo,
    descricao: opcoes.descricao ? readFileSync(opcoes.descricao, 'utf8') : undefined,
    branch: git(['rev-parse', '--abbrev-ref', 'HEAD']).trim(),
    commits,
  };
}

export async function main(argv = process.argv.slice(2), env = process.env, buscar = fetch) {
  const [modo, ...resto] = argv;
  const opcoes = lerArgumentos(resto);
  const regras = carregarRegras(opcoes.regras ?? REGRAS_PADRAO, env[VARIAVEL]);
  if (modo === 'comentario') {
    const evento = JSON.parse(readFileSync(env.GITHUB_EVENT_PATH, 'utf8'));
    const { resultado, achados } = await tratarComentario(env, env.GITHUB_EVENT_NAME, evento, regras, lerInternos(env[VARIAVEL]), buscar);
    console.log(formatar(achados, regras));
    console.log(`Comentário: ${resultado}`);
    return 0;
  }
  let entrada;
  if (modo === 'pr') entrada = await lerPr(env, buscar);
  else if (modo === 'local') entrada = lerLocal(opcoes);
  else throw new Error('Uso: ci-bastidor.mjs pr|comentario|local [--regras arquivo] [--base ref] [--titulo texto] [--descricao arquivo]');
  const achados = verificar(entrada, regras);
  console.log(formatar(achados, regras) || 'Sem bastidor.');
  return achados.length ? 1 : 0;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main().then((codigo) => { process.exitCode = codigo; }, (erro) => {
    console.log(`::error title=Bastidor::${erro.message}`);
    process.exitCode = 2;
  });
}
