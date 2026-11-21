// Trava contra bastidor em texto que vai ao GitHub (título e descrição de PR, nome de branch e commits).
// Node puro, sem dependências. Nunca imprime o trecho achado: só campo e linha, e a categoria das regras genéricas.
// O que casa com a lista interna aparece só como «texto interno encontrado», sem dizer qual termo.
// Modos: pr (lê o PR pela API) e local (antes de publicar). Guia: docs/guides/bastidor.md.
import { spawnSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const REGRAS_PADRAO = resolve(dirname(fileURLToPath(import.meta.url)), '../.github/security/bastidor.json');
const VARIAVEL = 'BASTIDOR_NOMES_INTERNOS';
const LIMITE_TEXTO = 2_000_000;
const LIMITE_ACHADOS = 200;
const LIMITE_ANOTACOES = 50;
const PRAZO_API_MS = 30_000;
// Só mensagens controladas chegam ao log: erro de parse ou de rede pode trazer pedaço do que lia.
const falha = (mensagem) => Object.assign(new Error(mensagem), { controlado: true });

export const normalizar = (texto) => texto
  .normalize('NFKD')
  .replace(/\p{M}/gu, '')
  .replace(/[\u00ad\u200b-\u200f\u2060\ufeff]/g, '')
  .replace(/&nbsp;/gi, ' ')
  .toLowerCase();

// Marcação (tags, ênfase, colchetes) não pode partir uma frase: cada linha é testada crua e sem marcação.
const semMarcacao = (linha) => linha
  .replace(/<(?!!--)[^>\n]{0,200}>/g, ' ')
  .replace(/[*`[\]~]/g, '')
  .replace(/(?<![a-z0-9])_|_(?![a-z0-9])/g, '');

const escapar = (texto) => texto.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');

function expandir(padrao, definicoes) {
  let atual = padrao;
  for (let i = 0; i < 5 && /\{\{\w+\}\}/.test(atual); i++) {
    atual = atual.replace(/\{\{(\w+)\}\}/g, (_, nome) => {
      if (!(nome in definicoes)) throw falha('Definição ausente nas regras');
      return definicoes[nome];
    });
  }
  if (/\{\{/.test(atual)) throw falha('Definição circular nas regras');
  return atual;
}

const listaDe = (valor, formato, limite = 50) => {
  if (!Array.isArray(valor) || valor.length > limite || !valor.every((v) => typeof v === 'string' && formato.test(v))) {
    throw falha(`${VARIAVEL} inválida`);
  }
  return valor;
};

// Formato do segredo: { prefixosTarefa: ["AB"], plataformas: ["nome"], termosInternos: ["termo"] }.
export function lerInternos(bruto) {
  if (bruto === undefined || bruto === null || !String(bruto).trim()) return null;
  let dados;
  try { dados = JSON.parse(bruto); } catch { throw falha(`${VARIAVEL} inválida`); }
  if (!dados || typeof dados !== 'object' || Array.isArray(dados)) throw falha(`${VARIAVEL} inválida`);
  if (Object.keys(dados).some((k) => !['prefixosTarefa', 'plataformas', 'termosInternos'].includes(k))) throw falha(`${VARIAVEL} inválida`);
  return {
    prefixosTarefa: listaDe(dados.prefixosTarefa ?? [], /^[A-Za-z]{2,8}$/),
    plataformas: listaDe(dados.plataformas ?? [], /^[A-Za-z0-9][A-Za-z0-9. -]{1,39}$/),
    termosInternos: listaDe(dados.termosInternos ?? [], /^[A-Za-z0-9][A-Za-z0-9. -]{1,39}$/),
  };
}

// Em PR do próprio repositório a lista interna é obrigatória: sem ela a proteção seria parcial e a checagem ficaria verde.
export function exigirInternos(bruto) {
  const internos = lerInternos(bruto);
  if (!internos) throw falha(`${VARIAVEL} ausente ou vazia: a checagem não roda sem a lista interna`);
  if (!internos.prefixosTarefa.length || !internos.plataformas.length) throw falha(`${VARIAVEL} sem prefixosTarefa ou plataformas`);
  return internos;
}

// O segredo inteiro já é mascarado pelo runner; cada termo também, para o caso de aparecer sozinho no log.
export const mascaras = (internos) => [...new Set(Object.values(internos).flat())].filter((v) => v.length >= 3).map((v) => `::add-mask::${v}`);

const alternativas = (lista) => lista.map((v) => escapar(normalizar(v).trim()).replace(/\s+/g, '\\s+')).join('|');

export function compilarRegras(config, internos = null) {
  if (config?.versao !== 1 || !Array.isArray(config.regras)) throw falha('Regras inválidas');
  const regras = config.regras.map((r) => ({
    id: r.id,
    categoria: r.categoria,
    campos: r.campos ?? null,
    interna: false,
    re: new RegExp(expandir(r.padrao, config.definicoes ?? {}), 'u'),
  }));
  const interna = (lista, corpo) => lista?.length && regras.push({ id: 'interno', categoria: 'interno', campos: null, interna: true, re: new RegExp(corpo(alternativas(lista)), 'u') });
  interna(internos?.prefixosTarefa, (a) => `(?<![a-z0-9])(?:${a})[-_]\\d{1,6}(?![a-z0-9])`);
  interna(internos?.plataformas, (a) => `(?<![a-z0-9])(?:${a})(?![a-z0-9])`);
  interna(internos?.termosInternos, (a) => `(?<![a-z0-9])(?:${a})(?![a-z0-9])`);
  return { regras, orientacao: config.orientacao ?? {}, completo: Boolean(internos) };
}

const rotuloCommit = (sha, indice) => `commit ${/^[0-9a-f]{7,40}$/i.test(sha ?? '') ? sha.slice(0, 7).toLowerCase() : indice + 1}`;

// Linha longa é testada em pedaços sobrepostos, com um caractere de contexto de cada lado: o custo cresce de forma
// linear, nenhum padrão vira laço quadrático e a borda do pedaço não satisfaz sozinha uma condição de fronteira.
const PEDACO = 2000;
const SOBRA = 300;
const pedacos = (linha) => {
  if (linha.length <= PEDACO) return [linha];
  const saida = [];
  for (let i = 0; i < linha.length; i += PEDACO - SOBRA) saida.push(linha.slice(Math.max(0, i - 1), Math.min(linha.length, i + PEDACO + 1)));
  return saida;
};

// entrada: { titulo, descricao, branch, commits: [{ sha, mensagem }] }. Devolve só posições, nunca o trecho.
export function verificar(entrada, { regras }) {
  const campos = [];
  if (entrada.titulo != null) campos.push(['titulo', entrada.titulo]);
  if (entrada.descricao != null) campos.push(['descricao', entrada.descricao]);
  if (entrada.branch != null) campos.push(['branch', entrada.branch]);
  (entrada.commits ?? []).forEach((c, i) => campos.push([rotuloCommit(c.sha, i), c.mensagem ?? '']));
  if (campos.reduce((soma, [, texto]) => soma + String(texto).length, 0) > LIMITE_TEXTO) throw falha('Texto grande demais para verificar');
  const achados = [];
  for (const [campo, texto] of campos) {
    const tipo = campo.split(' ')[0];
    const vistos = new Set();
    const linhas = normalizar(String(texto)).split(/\r\n|\r|\n/);
    for (let i = 0; i < linhas.length; i++) {
      const variantes = pedacos(linhas[i]).flatMap((pedaco) => [pedaco, semMarcacao(pedaco)]);
      for (const regra of regras) {
        if (regra.campos && !regra.campos.includes(tipo)) continue;
        const chave = `${regra.id}|${i}`;
        if (vistos.has(chave) || !variantes.some((v) => regra.re.test(v))) continue;
        vistos.add(chave);
        achados.push({ categoria: regra.categoria, regra: regra.id, campo, linha: i + 1 });
        if (achados.length >= LIMITE_ACHADOS) return achados;
      }
    }
  }
  return achados;
}

export function formatar(achados, { orientacao = {}, completo = true } = {}) {
  const linhas = [];
  if (!completo) {
    linhas.push(`::warning title=Bastidor::Checagem parcial: sem ${VARIAVEL}, códigos de tarefa, nome da plataforma e termos internos não foram verificados.`);
  }
  for (const a of achados.slice(0, LIMITE_ANOTACOES)) {
    linhas.push(a.categoria === 'interno'
      ? `::error title=Bastidor::texto interno encontrado: campo ${a.campo}, linha ${a.linha}`
      : `::error title=Bastidor::[${a.categoria}] campo ${a.campo}, linha ${a.linha} (regra ${a.regra})`);
  }
  if (achados.length > LIMITE_ANOTACOES) linhas.push('::error title=Bastidor::há outros achados não listados');
  const dicas = [...new Set(achados.map((a) => orientacao[a.categoria]).filter(Boolean))];
  for (const dica of dicas) linhas.push(dica);
  if (achados.length) linhas.push('Reescreva só com o que mudou, por quê, como foi testado e o que ficou sem provar. Guia: docs/guides/bastidor.md.');
  return linhas.join('\n');
}

async function api(env, caminho, buscar = fetch) {
  const resposta = await buscar(`${env.GITHUB_API_URL ?? 'https://api.github.com'}${caminho}`, {
    headers: {
      Authorization: `Bearer ${env.GH_TOKEN ?? env.GITHUB_TOKEN}`,
      Accept: 'application/vnd.github+json',
      'X-GitHub-Api-Version': '2022-11-28',
    },
    signal: AbortSignal.timeout(PRAZO_API_MS),
  });
  if (!resposta.ok) throw falha(`API recusou a leitura (${resposta.status})`);
  return resposta.json();
}

const repositorio = (env) => {
  if (!/^(?!\.{1,2}\/)[\w.-]+\/(?!\.{1,2}$)[\w.-]+$/.test(env.GITHUB_REPOSITORY ?? '')) throw falha('GITHUB_REPOSITORY inválido');
  return env.GITHUB_REPOSITORY;
};

// Fotografia consistente: o SHA do evento é fixado no início, e título, descrição, branch, SHA e número de commits são
// relidos no fim; se algo mudou, a execução reprova e a rodada do evento seguinte decide.
export async function lerPr(env, buscar = fetch) {
  const repo = repositorio(env);
  const numero = Number(env.PR_NUMBER);
  if (!Number.isInteger(numero) || numero < 1) throw falha('PR_NUMBER inválido');
  if (!/^[a-f0-9]{40}$/.test(env.PR_HEAD_SHA ?? '')) throw falha('PR_HEAD_SHA inválido');
  const ler = async () => {
    const pr = await api(env, `/repos/${repo}/pulls/${numero}`, buscar);
    if (typeof pr?.title !== 'string' || typeof pr.head?.ref !== 'string' || typeof pr.head.sha !== 'string' || !Number.isInteger(pr.commits) || (pr.body != null && typeof pr.body !== 'string')) throw falha('Resposta da API sem os campos do PR');
    return pr;
  };
  const mudou = () => falha('O PR mudou durante a verificação: a execução mais nova verifica o estado atual');
  const pr = await ler();
  if (pr.head.sha !== env.PR_HEAD_SHA) throw mudou();
  const commits = [];
  for (let pagina = 1; pagina <= 3; pagina++) {
    const lote = await api(env, `/repos/${repo}/pulls/${numero}/commits?per_page=100&page=${pagina}`, buscar);
    if (!Array.isArray(lote) || lote.some((c) => typeof c?.commit?.message !== 'string')) throw falha('Resposta da API sem as mensagens de commit');
    commits.push(...lote.map((c) => ({ sha: c.sha, mensagem: c.commit.message })));
    if (lote.length < 100) break;
  }
  if (commits.length < pr.commits) throw falha('PR com mais commits do que a API lista: confira localmente');
  const foto = (x) => JSON.stringify([x.title, x.body ?? '', x.head.ref, x.head.sha, x.commits]);
  if (foto(await ler()) !== foto(pr)) throw mudou();
  return { titulo: pr.title, descricao: pr.body ?? '', branch: pr.head.ref, commits };
}

function lerArgumentos(argv) {
  const opcoes = {};
  for (let i = 0; i < argv.length; i++) {
    if (!argv[i].startsWith('--') || i + 1 >= argv.length) throw falha('Argumento inválido');
    opcoes[argv[i].slice(2)] = argv[++i];
  }
  return opcoes;
}

const git = (args) => {
  const r = spawnSync('git', ['--no-pager', ...args], { encoding: 'utf8', maxBuffer: 64 * 1024 * 1024 });
  if (r.status !== 0) throw falha('Operação Git recusada');
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
  if (!['pr', 'local'].includes(modo)) throw falha('Uso: ci-bastidor.mjs pr|local [--regras arquivo] [--base ref] [--titulo texto] [--descricao arquivo]');
  // PR de fork roda só com as regras genéricas e nunca recebe a lista interna: o resultado público não pode servir de
  // oráculo para ela. PR do próprio repositório exige a lista; o modo local pode rodar parcial e avisa.
  if (modo === 'pr' && !['true', 'false'].includes(env.BASTIDOR_FORK)) throw falha('BASTIDOR_FORK inválido');
  const fork = modo === 'pr' && env.BASTIDOR_FORK === 'true';
  const internos = fork ? null : modo === 'local' ? lerInternos(env[VARIAVEL]) : exigirInternos(env[VARIAVEL]);
  if (internos && modo === 'pr') for (const linha of mascaras(internos)) console.log(linha);
  const regras = compilarRegras(JSON.parse(readFileSync(opcoes.regras ?? REGRAS_PADRAO, 'utf8')), internos);
  if (fork) console.log('PR de fork: só as regras genéricas foram aplicadas.');
  const entrada = modo === 'pr' ? await lerPr(env, buscar) : lerLocal(opcoes);
  const achados = verificar(entrada, regras);
  console.log(formatar(achados, { ...regras, completo: regras.completo || fork }) || 'Sem bastidor.');
  return achados.length ? 1 : 0;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main().then((codigo) => { process.exitCode = codigo; }, (erro) => {
    console.log(`::error title=Bastidor::${erro.controlado ? erro.message : 'Falha inesperada na verificação'}`);
    process.exitCode = 2;
  });
}
