import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, readFileSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';
import { compilarRegras, lerInternos, verificar, formatar, normalizar, tratarComentario, lerPr, main } from './ci-bastidor.mjs';

const arquivoRegras = resolve('.github/security/bastidor.json');
const config = JSON.parse(readFileSync(arquivoRegras, 'utf8'));
const brutoInternos = JSON.stringify({
  prefixosTarefa: ['ZZ'],
  plataformas: ['plataforma-ficticia'],
  termosInternos: ['termo-ficticio'],
  automacao: ['robo-ficticio[bot]'],
});
const internos = lerInternos(brutoInternos);
const genericas = compilarRegras(config, null);
const completas = compilarRegras(config, internos);
const ids = (entrada, regras = completas) => verificar(entrada, regras).map((a) => a.regra);

// Títulos reais de PRs abertos: nome de produto como assunto da mudança tem de passar.
const TITULOS_REAIS = [
  "docs(security): relatar pelo Private Vulnerability Reporting como canal principal",
  "docs(readme): avisar o estado real do projeto e listar só o que de fato não está aqui",
  "ci: menos máquinas por rodada e máquina escolhida por variável do repositório",
  "ci(macos): testar e montar a Open no Mac a cada PR, num Mac de nuvem",
  "fix(hub): Kilo e Cline passam pelo cartão de aprovação do porteiro",
  "fix(motor): a conversa do Motor Mexerica sobrevive ao processo",
  "feat(host): Claude por assinatura passa a rodar pelo programa oficial da Anthropic, sem o SDK de agentes",
  "fix(hub): fila de mensagens sem duplicata garantida e com limite de 50 por conversa",
  "feat(contas): conectar o Grok por assinatura pelo Grok Build, com o código na tela",
  "fix(hub): resposta cortada por desligamento ou queda chega inteira e marcada como interrompida",
  "feat(host): Claude, Codex, OpenCode, Copilot e Qwen instalam por npm com versão fixada",
  "feat(motor): chaves de API no Motor Mexerica, com «Testar acesso» e a lista de modelos do fornecedor",
  "feat(rotinas): Rotina «só quando eu mandar», um botão com nome",
  "test(rotinas): cenários de ponta a ponta da checagem silenciosa",
  "feat(instrucoes): campos «Para todos os meus bots» e «Neste Space»",
  "feat(contexto): mostra o peso de cada parte e o uso real do turno",
  "feat(rotinas): a checagem silenciosa consulta a fonte no agendador",
  "feat(host): instalação por script do fornecedor passa a pedir confirmação",
  "fix(motor): resposta do fornecedor não fica mais pendurada no Motor Mexerica",
  "fix(motor): o Motor Mexerica passa a receber as instruções fixas do bot",
  "feat(desktop): mostrar gráfico de leitura com «Ver dados» no chat",
  "feat(host): mostrar gráfico de leitura no chat do Berga",
  "feat(hub): avisar na conversa quando o motor do bot fica mudo no meio de um turno",
  "feat(host): planilha criada no computador escolhido, com recibo conferido",
  "feat(navegador): abrir e ler páginas HTTPS com sessão isolada por bot",
  "feat(desktop): consome os quadros binários do navegador do bot (B2)",
  "test(berga): ensaio offline dos avisos proativos e do silêncio"
];

const REPROVA = [
  ['selo-html', { descricao: 'Texto\n<!-- x-badge:start -->\nselo\n<!-- x-badge:end -->' }, 2],
  ['id-conversa', { descricao: 'conversa jam_0123456789ABCDEF' }, 1],
  ['link-conversa', { descricao: 'veja https://app.example.invalid/thread/abc123def456' }, 1],
  ['link-ferramenta-ia', { descricao: 'veja https://claude.ai/share/abc' }, 1],
  ['selo-robo', { descricao: '\u{1F916} Generated with algo' }, 1],
  ['tarefa-palavra-chave', { descricao: 'Closes AB-123' }, 1],
  ['credito-autoria', { descricao: 'Feito com Claude' }, 1],
  ['credito-autoria', { descricao: 'texto\nfeito por Sonnet 5.5' }, 2],
  ['credito-autoria', { descricao: 'Escrito pelo Codex' }, 1],
  ['credito-autoria', { descricao: 'Built with Cursor' }, 1],
  ['credito-autoria', { descricao: 'feito com IA' }, 1],
  ['credito-geracao', { descricao: 'Generated with Claude Code' }, 1],
  ['credito-geracao', { descricao: 'Gerado com Gemini' }, 1],
  ['credito-revisao', { descricao: 'Revisado por Gemini 4 Neon' }, 1],
  ['credito-revisao', { descricao: 'revisado pelo Codex' }, 1],
  ['credito-revisao', { descricao: 'Audited by GPT-6.2 Sol' }, 1],
  ['revisao-por-agente', { descricao: 'revisado por um agente' }, 1],
  ['sujeito-verbo', { descricao: 'Opus revisou o código' }, 1],
  ['rotulo-autoria', { descricao: '- Revisão: Sonnet' }, 1],
  ['rotulo-autoria', { descricao: 'Autor: Claude' }, 1],
  ['revisao-do-modelo', { descricao: 'Revisão independente do Neon' }, 1],
  ['revisao-do-modelo', { descricao: 'segunda opinião do Sol' }, 1],
  ['trailer-ia', { commits: [{ sha: 'abc1234567', mensagem: 'ajuste\n\nCo-Authored-By: Claude <noreply@anthropic.com>' }] }, 3],
  ['trailer-ia', { commits: [{ sha: 'abc1234567', mensagem: 'ajuste\n\nReviewed-by: Neon' }] }, 3],
  ['ajuda-de-ia', { descricao: 'feito com ajuda do Claude' }, 1],
  ['assistido-por-ia', { descricao: 'AI-assisted' }, 1],
  ['branch-ferramenta-ia', { branch: 'claude/ajuste-simples' }, 1],
  ['subagente', { descricao: 'o subagente terminou' }, 1],
  ['decisao-do-dono', { descricao: 'o dono decidiu usar isto' }, 1],
  ['decisao-do-dono', { descricao: 'Decisão do dono: seguir' }, 1],
  ['decisao-do-dono', { descricao: 'quem coordena a frente' }, 1],
  ['relatorio-interno', { descricao: 'está no relatório interno' }, 1],
  ['relatorio-interno', { descricao: 'veja drive://reports/x.md' }, 1],
  ['thread-da-equipe', { descricao: 'na thread pai' }, 1],
  ['custo-interno', { descricao: 'custo da rodada' }, 1],
  ['custo-interno', { descricao: 'gastamos US$ 12 nesta rodada' }, 1],
  ['custo-interno', { descricao: '12 créditos gastos' }, 1],
  ['contagem-maquinas', { descricao: 'ligou 2.360 máquinas em 217 rodadas' }, 1],
  ['contagem-maquinas', { descricao: 'limite de 60 inícios por hora' }, 1],
  ['contagem-maquinas', { descricao: 'a contagem de máquinas' }, 1],
  ['tarefa-codigo', { descricao: 'veja ZZ-232' }, 1],
  ['tarefa-codigo', { branch: 'feat/zz-232-ajuste' }, 1],
  ['tarefa-codigo', { commits: [{ sha: 'abc1234567', mensagem: 'ajuste ZZ_12' }] }, 1],
  ['plataforma-nome', { descricao: 'abra em plataforma-ficticia' }, 1],
  ['plataforma-nome', { descricao: 'https://app.plataforma-ficticia.example.invalid/x' }, 1],
  ['termo-interno', { descricao: 'o termo-ficticio decidiu' }, 1],
];

const PASSA = [
  'conectar o Codex',
  'motor Cline',
  'Claude pelo programa oficial',
  'Claude por assinatura passa a rodar pelo programa oficial da Anthropic',
  'Testado com Claude, Codex e Cline: a conexão abre e fecha',
  'O Grok Build mostra o código na tela',
  'Fixes CVE-2026-1234, closes #12, refs UTF-8',
  'Revisado por Maria e por João',
  'Co-authored-by: Maria <maria@example.invalid>',
  'revisão de segurança do porteiro: o cartão pede aprovação',
  'O turno custou US$ 0,02 e mostra o uso por parte',
  'o dono do dispositivo aprova o pedido no cartão',
  'a thread principal do Electron não bloqueia',
  'Feito com carinho',
  'sem agente de revisão externo no fluxo',
];

test('o arquivo de regras compila e cada regra tem exemplo que reprova', () => {
  const vistos = new Set(REPROVA.map(([regra]) => regra));
  const esperados = [...config.regras.map((r) => r.id), 'tarefa-codigo', 'plataforma-nome', 'termo-interno'];
  assert.deepEqual([...vistos].sort(), esperados.sort());
  for (const [regra, entrada, linha] of REPROVA) {
    const achados = verificar(entrada, completas).filter((a) => a.regra === regra);
    assert.ok(achados.length > 0, 'não reprovou: ' + regra);
    assert.equal(achados[0].linha, linha, 'linha de ' + regra);
  }
});

test('nome de produto como assunto e texto comum passam', () => {
  for (const texto of PASSA) {
    assert.deepEqual(verificar({ titulo: texto, descricao: texto }, completas), [], texto);
    assert.deepEqual(verificar({ titulo: texto, descricao: texto }, genericas), [], texto);
  }
});

test('títulos reais dos PRs abertos passam', () => {
  assert.equal(TITULOS_REAIS.length, 27);
  for (const titulo of TITULOS_REAIS) {
    assert.deepEqual(verificar({ titulo, descricao: titulo }, completas), [], titulo);
    assert.deepEqual(verificar({ titulo }, genericas), [], titulo);
  }
});

test('descrição e commits comuns de um PR passam', () => {
  const descricao = '## Problema\n\nO Claude por assinatura rodava pelo SDK.\n\n## Solução\n\n- passa a rodar pelo programa oficial\n- o Codex e o Cline seguem iguais\n\n## Teste\n\n- cargo test\n- Testado com Codex\n\nCloses #12\n';
  const commits = [{ sha: 'abcdef1234', mensagem: 'feat(host): programa oficial\n\nMantém o Codex.' }];
  assert.deepEqual(verificar({ titulo: 'feat(host): programa oficial', descricao, branch: 'feat/programa-oficial', commits }, completas), []);
});

test('sem a variável só valem os genéricos e o aviso aparece', () => {
  const achados = verificar({ branch: 'feat/zz-232-ajuste', descricao: 'abra em plataforma-ficticia' }, genericas);
  assert.deepEqual(achados, []);
  assert.match(formatar(achados, genericas), /::warning title=Bastidor::.*BASTIDOR_NOMES_INTERNOS/);
  assert.ok(!formatar([], completas).includes('::warning'));
});

test('a variável inválida falha sem repetir o conteúdo', () => {
  assert.equal(lerInternos(undefined), null);
  assert.equal(lerInternos('   '), null);
  for (const bruto of ['nao e json segredo-ficticio', '[]', '{"outra":["segredo-ficticio"]}', '{"prefixosTarefa":"ZZ"}', '{"prefixosTarefa":["Z"]}', '{"automacao":["a b"]}']) {
    assert.throws(() => lerInternos(bruto), (e) => /inválida/.test(e.message) && !e.message.includes('segredo-ficticio'));
  }
});

test('a saída diz categoria, campo e linha e nunca repete o trecho', () => {
  const entrada = {
    titulo: 'ajuste',
    descricao: 'ok\nFeito com Claude Code\nrevisado pelo Codex e abra em plataforma-ficticia',
    branch: 'feat/zz-232-x',
    commits: [{ sha: 'ABCDEF1234567', mensagem: 'ajuste\n\nCo-Authored-By: Claude <noreply@anthropic.com>' }],
  };
  const saida = formatar(verificar(entrada, completas), completas);
  assert.match(saida, /\[autoria-ia\] campo descricao, linha 2 \(regra credito-autoria\)/);
  assert.match(saida, /\[autoria-ia\] campo descricao, linha 3 \(regra credito-revisao\)/);
  assert.match(saida, /\[plataforma\] campo descricao, linha 3/);
  assert.match(saida, /\[tarefa\] campo branch, linha 1/);
  assert.match(saida, /campo commit abcdef1, linha 3 \(regra trailer-ia\)/);
  const baixa = saida.toLowerCase();
  for (const proibido of ['claude', 'codex', 'anthropic', 'plataforma-ficticia', 'zz-232', 'termo-ficticio', 'noreply']) {
    assert.ok(!baixa.includes(proibido), 'saída repetiu: ' + proibido);
  }
  assert.ok(saida.split('\n').every((l) => !l.startsWith('::') || /^::(?:error|warning) title=Bastidor::/.test(l)));
});

test('marcação, espaço especial e caracteres invisíveis não escondem o texto', () => {
  const crase = String.fromCharCode(96);
  const evasoes = [
    '**Feito com** _Claude_',
    'Feito com ' + crase + 'Claude' + crase,
    '<b>Feito com</b> <i>Claude</i>',
    'Feito com [Claude](https://example.invalid)',
    'Feito&nbsp;com Claude',
    'Fe\u200bito com Cl\u00adaude',
    'FEITO COM CLAUDE',
    '\uFF26\uFF45\uFF49\uFF54\uFF4F \uFF43\uFF4F\uFF4D \uFF23\uFF4C\uFF41\uFF55\uFF44\uFF45',
    'Revisão independente do Neon',
  ];
  for (const texto of evasoes) assert.ok(ids({ descricao: texto }).length > 0, texto);
  assert.equal(normalizar('Revisão ÁGUA'), 'revisao agua');
});

test('commits: o rótulo usa o início do hash ou a posição', () => {
  const achados = verificar({ commits: [{ sha: 'a'.repeat(40), mensagem: 'limpo' }, { sha: 'b'.repeat(40), mensagem: 'x\nfeito com Claude' }, { mensagem: 'revisado por Opus' }] }, completas);
  assert.deepEqual(achados.map((a) => a.campo + ':' + a.linha), ['commit bbbbbbb:2', 'commit 3:1']);
});

function resposta(status, corpo) {
  return { ok: status >= 200 && status < 300, status, json: async () => corpo };
}

function apiFalsa({ pr, commits = [], permissao = 'write' }) {
  const chamadas = [];
  const corpos = [];
  const buscar = async (url, opcoes = {}) => {
    const alvo = new URL(url);
    const rota = (opcoes.method ?? 'GET') + ' ' + alvo.pathname;
    chamadas.push(rota);
    if (opcoes.body) corpos.push(opcoes.body);
    if (rota === 'GET /repos/o/r/pulls/7') return resposta(200, pr);
    if (rota === 'GET /repos/o/r/pulls/7/commits') {
      const pagina = Number(alvo.searchParams.get('page'));
      return resposta(200, commits.slice((pagina - 1) * 100, pagina * 100));
    }
    if (rota.startsWith('GET /repos/o/r/collaborators/')) return resposta(200, { permission: permissao });
    if (rota.startsWith('DELETE ')) return resposta(204, null);
    if (rota.startsWith('POST /repos/')) return resposta(201, {});
    if (rota === 'POST /api/graphql' || rota === 'POST /graphql') return resposta(200, { data: {} });
    return resposta(404, {});
  };
  return { buscar, chamadas, corpos };
}

const AMBIENTE = { GITHUB_REPOSITORY: 'o/r', GITHUB_API_URL: 'https://api.example.invalid', GH_TOKEN: 'token-ficticio', PR_NUMBER: '7' };
const commit = (i, mensagem = 'ajuste ' + i) => ({ sha: String(i).padStart(40, '0'), commit: { message: mensagem } });

async function capturar(fn) {
  const original = console.log;
  const linhas = [];
  console.log = (...args) => linhas.push(args.join(' '));
  try { return { codigo: await fn(), saida: linhas.join('\n') }; } finally { console.log = original; }
}

test('modo pr lê título, descrição, branch e todas as páginas de commits pela API', async () => {
  const commits = Array.from({ length: 101 }, (_, i) => commit(i + 1));
  commits[100] = commit(101, 'x\nfeito com Claude');
  const api = apiFalsa({ pr: { title: 'ajuste', body: 'texto limpo', head: { ref: 'feat/ajuste' }, commits: 101 }, commits });
  const entrada = await lerPr(AMBIENTE, api.buscar);
  assert.equal(entrada.commits.length, 101);
  assert.equal(api.chamadas.filter((c) => c.endsWith('/commits')).length, 2);
  const { codigo, saida } = await capturar(() => main(['pr', '--regras', arquivoRegras], { ...AMBIENTE, BASTIDOR_NOMES_INTERNOS: brutoInternos }, api.buscar));
  assert.equal(codigo, 1);
  assert.match(saida, /campo commit 0000000, linha 2 \(regra credito-autoria\)/);
  assert.ok(!saida.toLowerCase().includes('claude'));
});

test('modo pr passa limpo, reprova título e branch e falha fechado quando a API não lista tudo', async () => {
  const env = { ...AMBIENTE, BASTIDOR_NOMES_INTERNOS: brutoInternos };
  const limpo = apiFalsa({ pr: { title: 'feat(host): conectar o Codex', body: null, head: { ref: 'feat/codex' }, commits: 1 }, commits: [commit(1)] });
  assert.equal((await capturar(() => main(['pr', '--regras', arquivoRegras], env, limpo.buscar))).codigo, 0);
  const sujo = apiFalsa({ pr: { title: 'Feito com Claude', body: '', head: { ref: 'feat/zz-1-x' }, commits: 1 }, commits: [commit(1)] });
  const { codigo, saida } = await capturar(() => main(['pr', '--regras', arquivoRegras], env, sujo.buscar));
  assert.equal(codigo, 1);
  assert.match(saida, /campo titulo, linha 1/);
  assert.match(saida, /campo branch, linha 1/);
  const incompleto = apiFalsa({ pr: { title: 'a', body: '', head: { ref: 'b' }, commits: 400 }, commits: [commit(1)] });
  await assert.rejects(lerPr(AMBIENTE, incompleto.buscar), /mais commits/);
  await assert.rejects(lerPr({ ...AMBIENTE, PR_NUMBER: '7; rm' }, limpo.buscar), /PR_NUMBER/);
  await assert.rejects(lerPr({ ...AMBIENTE, GITHUB_REPOSITORY: '../x' }, limpo.buscar), /GITHUB_REPOSITORY/);
});

const humano = (login = 'pessoa-ficticia') => ({ login, type: 'User' });
const eventoIssue = (corpo, usuario = humano(), action = 'created') => ({ action, issue: { number: 7 }, comment: { id: 55, node_id: 'NO_1', body: corpo, user: usuario } });

test('comentário de quem tem escrita com bastidor é removido e recebe aviso neutro', async () => {
  const api = apiFalsa({ pr: {}, permissao: 'write' });
  const r = await tratarComentario(AMBIENTE, 'issue_comment', eventoIssue('Feito com Claude'), completas, internos, api.buscar);
  assert.equal(r.resultado, 'removido');
  assert.deepEqual(api.chamadas, ['GET /repos/o/r/collaborators/pessoa-ficticia/permission', 'DELETE /repos/o/r/issues/comments/55', 'POST /repos/o/r/issues/7/comments']);
  const aviso = api.corpos.join('');
  assert.ok(!aviso.toLowerCase().includes('claude'));
  assert.match(aviso, /categoria: autoria-ia/);
});

test('comentário de revisão em linha é removido e corpo de revisão enviada fica oculto', async () => {
  const linha = apiFalsa({ pr: {} });
  const evento = { action: 'created', pull_request: { number: 7 }, comment: { id: 66, node_id: 'NO_2', body: 'revisado por Opus', user: humano() } };
  assert.equal((await tratarComentario(AMBIENTE, 'pull_request_review_comment', evento, completas, internos, linha.buscar)).resultado, 'removido');
  assert.ok(linha.chamadas.includes('DELETE /repos/o/r/pulls/comments/66'));
  const revisao = apiFalsa({ pr: {} });
  const evento2 = { action: 'submitted', pull_request: { number: 7 }, review: { id: 77, node_id: 'NO_3', body: 'Revisão: Sonnet', user: humano() } };
  assert.equal((await tratarComentario(AMBIENTE, 'pull_request_review', evento2, completas, internos, revisao.buscar)).resultado, 'ocultado');
  assert.ok(revisao.chamadas.includes('POST /graphql'));
  assert.ok(!revisao.chamadas.some((c) => c.startsWith('DELETE')));
});

test('gente de fora, robô de terceiro, texto limpo e evento apagado não são tocados', async () => {
  const semEscrita = apiFalsa({ pr: {}, permissao: 'read' });
  assert.equal((await tratarComentario(AMBIENTE, 'issue_comment', eventoIssue('Feito com Claude'), completas, internos, semEscrita.buscar)).resultado, 'fora-do-escopo');
  assert.ok(!semEscrita.chamadas.some((c) => /^(DELETE|POST)/.test(c)));
  const terceiro = apiFalsa({ pr: {} });
  const bot = { login: 'revisor-externo[bot]', type: 'Bot' };
  assert.equal((await tratarComentario(AMBIENTE, 'issue_comment', eventoIssue('Feito com Claude', bot), completas, internos, terceiro.buscar)).resultado, 'fora-do-escopo');
  assert.deepEqual(terceiro.chamadas, []);
  const limpo = apiFalsa({ pr: {} });
  assert.equal((await tratarComentario(AMBIENTE, 'issue_comment', eventoIssue('Conectar o Codex'), completas, internos, limpo.buscar)).resultado, 'limpo');
  assert.equal((await tratarComentario(AMBIENTE, 'issue_comment', eventoIssue('Feito com Claude', humano(), 'deleted'), completas, internos, limpo.buscar)).resultado, 'ignorado');
  assert.equal((await tratarComentario(AMBIENTE, 'issue_comment', eventoIssue(''), completas, internos, limpo.buscar)).resultado, 'ignorado');
  assert.equal((await tratarComentario(AMBIENTE, 'push', {}, completas, internos, limpo.buscar)).resultado, 'ignorado');
  assert.deepEqual(limpo.chamadas, []);
});

test('a automação da lista da variável tem o comentário removido; sem lista, nenhum robô; login estranho é recusado', async () => {
  const bot = { login: 'robo-ficticio[bot]', type: 'Bot' };
  const api = apiFalsa({ pr: {} });
  assert.equal((await tratarComentario(AMBIENTE, 'issue_comment', eventoIssue('Feito com Claude', bot), completas, internos, api.buscar)).resultado, 'removido');
  assert.ok(!api.chamadas.some((c) => c.includes('/collaborators/')));
  const sem = apiFalsa({ pr: {} });
  assert.equal((await tratarComentario(AMBIENTE, 'issue_comment', eventoIssue('Feito com Claude', bot), genericas, null, sem.buscar)).resultado, 'fora-do-escopo');
  const injecao = apiFalsa({ pr: {} });
  assert.equal((await tratarComentario(AMBIENTE, 'issue_comment', eventoIssue('Feito com Claude', humano('x/../y')), completas, internos, injecao.buscar)).resultado, 'fora-do-escopo');
  assert.deepEqual(injecao.chamadas, []);
});

test('falha da API na permissão ou na remoção reprova o job em vez de fingir sucesso', async () => {
  const quebrada = async () => resposta(500, {});
  await assert.rejects(tratarComentario(AMBIENTE, 'issue_comment', eventoIssue('Feito com Claude'), completas, internos, quebrada), /API recusou/);
});

test('modo local usa o branch, os commits da base e o título informado', () => {
  const pasta = mkdtempSync(join(tmpdir(), 'bastidor-'));
  try {
    const env = { PATH: process.env.PATH, HOME: pasta, GIT_CONFIG_NOSYSTEM: '1', GIT_CONFIG_GLOBAL: '/dev/null' };
    const git = (...args) => { const r = spawnSync('git', args, { cwd: pasta, encoding: 'utf8', env }); assert.equal(r.status, 0); return r.stdout.trim(); };
    git('init', '-q', '-b', 'main'); git('config', 'user.name', 'Teste'); git('config', 'user.email', 'teste@example.invalid');
    writeFileSync(join(pasta, 'a.txt'), 'a'); git('add', '.'); git('-c', 'core.hooksPath=/dev/null', 'commit', '-qm', 'base');
    const base = git('rev-parse', 'HEAD');
    git('checkout', '-q', '-b', 'feat/ajuste');
    writeFileSync(join(pasta, 'b.txt'), 'b'); git('add', '.'); git('-c', 'core.hooksPath=/dev/null', 'commit', '-qm', 'ajuste simples');
    const rodar = (...extra) => spawnSync(process.execPath, [resolve('tools/ci-bastidor.mjs'), 'local', '--regras', arquivoRegras, '--base', base, ...extra], { cwd: pasta, encoding: 'utf8', env });
    assert.equal(rodar('--titulo', 'feat: conectar o Codex').status, 0);
    const ruim = rodar('--titulo', 'Feito com Claude');
    assert.equal(ruim.status, 1);
    assert.match(ruim.stdout, /campo titulo, linha 1/);
    assert.ok(!ruim.stdout.toLowerCase().includes('claude'));
    writeFileSync(join(pasta, 'c.txt'), 'c'); git('add', '.'); git('-c', 'core.hooksPath=/dev/null', 'commit', '-qm', 'outro\n\nRevisado por Opus');
    const commitRuim = rodar();
    assert.equal(commitRuim.status, 1);
    assert.match(commitRuim.stdout, /campo commit [0-9a-f]{7}, linha 3 \(regra credito-revisao\)/);
    assert.equal(rodar('--base', 'ref-que-nao-existe').status, 2);
  } finally { rmSync(pasta, { recursive: true, force: true }); }
});

const lerWorkflow = (nome) => readFileSync(new URL('../.github/workflows/' + nome, import.meta.url), 'utf8');
const WORKFLOWS = { 'bastidor.yml': lerWorkflow('bastidor.yml'), 'bastidor-comentarios.yml': lerWorkflow('bastidor-comentarios.yml') };

test('workflows: só ASCII, permissões mínimas, actions fixadas por SHA e sem checkout', () => {
  for (const [nome, texto] of Object.entries(WORKFLOWS)) {
    assert.ok(!/[^\x00-\x7F]/.test(texto), nome + ' deve ser ASCII');
    assert.match(texto, /^permissions: \{\}$/m, nome);
    for (const proibido of ['pull_request_target', 'workflow_run', 'schedule', 'push:', 'workflow_dispatch', 'merge_group', 'actions/checkout', 'container:', 'services:']) {
      assert.ok(!texto.includes(proibido), nome + ': ' + proibido);
    }
    const usos = [...texto.matchAll(/^\s+(?:- )?uses: (\S+)/gm)].map((m) => m[1]);
    assert.deepEqual(usos, ['actions/setup-node@949feb2413d6458794dcd2491c4babbbce0c15c1'], nome);
    assert.equal((texto.match(/^    runs-on: (.+)$/gm) ?? []).length, 1);
    assert.ok(texto.includes("    runs-on: ${{ vars.CI_MAQUINA_LEVE || 'instabox-small' }}"), nome);
    assert.ok(!/ubuntu-|windows-|macos-|self-hosted|boat|nscloud/i.test(texto), nome + ': rótulo de máquina fora do padrão');
    assert.match(texto, /^    timeout-minutes: [1-9][0-9]*$/m, nome);
    assert.deepEqual([...new Set([...texto.matchAll(/\bvars\.(\w+)/g)].map((m) => m[1]))].sort(), ['BASTIDOR_NOMES_INTERNOS', 'CI_MAQUINA_LEVE'], nome);
    assert.match(texto, /if: \$\{\{ runner\.environment == 'github-hosted' && github\.event\.repository\.private \}\}/, nome + ': recusa máquina do GitHub em repositório privado');
  }
});

test('workflows: texto de PR e de comentário nunca entra em comando nem em expressão de execução', () => {
  for (const [nome, texto] of Object.entries(WORKFLOWS)) {
    const linhas = texto.split('\n');
    linhas.forEach((linha, i) => {
      const bloco = /^(\s+)run: (.*)$/.exec(linha);
      if (!bloco) return;
      const corpo = [bloco[2]];
      if (bloco[2].trim() === '|') {
        for (let j = i + 1; j < linhas.length && (linhas[j].trim() === '' || linhas[j].startsWith(bloco[1] + ' '));) corpo.push(linhas[j++]);
      }
      assert.ok(!corpo.join('\n').includes('${{'), nome + ': expressão dentro de run na linha ' + (i + 1));
    });
    assert.ok(!/github\.(head_ref|event\.(pull_request\.(title|body|head\.ref)|comment\.body|review\.body|issue\.(title|body)))/.test(texto), nome + ': texto livre do evento');
  }
});

test('workflow de PR: só os quatro eventos, leitura apenas e verificador da branch padrão', () => {
  const texto = WORKFLOWS['bastidor.yml'];
  assert.match(texto, /^on:\n  pull_request:\n    types: \[opened, edited, reopened, synchronize\]\n\n/m);
  assert.ok(!/: write\b/.test(texto));
  assert.ok(texto.includes('if: ${{ github.event.pull_request.head.repo.full_name == github.repository }}'));
  assert.match(texto, /group: bastidor-\$\{\{ github\.event\.pull_request\.number \}\}\n  cancel-in-progress: true/);
  assert.ok(texto.includes('baixar "$DEFAULT_BRANCH" tools/ci-bastidor.mjs'));
  assert.match(texto, /\[ "\$codigo" = 404 \]/);
  assert.ok(texto.includes('node "$RUNNER_TEMP/bastidor/ci-bastidor.mjs" pr'));
});

test('workflow de comentários: três eventos, escrita só em issues e pull-requests, robô de terceiro não liga máquina', () => {
  const texto = WORKFLOWS['bastidor-comentarios.yml'];
  assert.match(texto, /^on:\n  issue_comment:\n    types: \[created, edited\]\n  pull_request_review:\n    types: \[submitted, edited\]\n  pull_request_review_comment:\n    types: \[created, edited\]\n\n/m);
  assert.deepEqual([...texto.matchAll(/^      ([a-z-]+): (read|write)$/gm)].map((m) => m[1] + ':' + m[2]), ['contents:read', 'issues:write', 'pull-requests:write']);
  const condicao = /^    if: >-\n((?:      .+\n)+)/m.exec(texto)[1];
  for (const trecho of ['OWNER', 'MEMBER', 'COLLABORATOR', 'github-actions[bot]', 'head.repo.full_name == github.repository', "fromJSON(vars.BASTIDOR_NOMES_INTERNOS || '{}').automacao"]) {
    assert.ok(condicao.includes(trecho), 'condição sem ' + trecho);
  }
  assert.ok(!texto.includes('PR_HEAD_SHA'), 'comentário nunca roda verificador vindo do PR');
  assert.ok(texto.includes('node "$RUNNER_TEMP/bastidor/ci-bastidor.mjs" comentario'));
});

test('o contrato dos workflows e o teste do verificador rodam no CI de ferramentas', () => {
  const ci = readFileSync(new URL('../.github/workflows/ci.yml', import.meta.url), 'utf8');
  assert.ok(ci.includes('tools/ci-bastidor.test.mjs'));
});
