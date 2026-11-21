import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, readFileSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';
import { compilarRegras, lerInternos, exigirInternos, mascaras, verificar, formatar, normalizar, lerPr, main } from './ci-bastidor.mjs';

const arquivoRegras = resolve('.github/security/bastidor.json');
const config = JSON.parse(readFileSync(arquivoRegras, 'utf8'));
const brutoInternos = JSON.stringify({
  prefixosTarefa: ['ZZ'],
  plataformas: ['plataforma-ficticia'],
  termosInternos: ['termo-ficticio'],
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
  ['credito-geracao', { descricao: 'Gerado por IA' }, 1],
  ['credito-geracao', { descricao: 'Gerado pelo Claude' }, 1],
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
  ['interno', { descricao: 'veja ZZ-232' }, 1],
  ['interno', { branch: 'feat/zz-232-ajuste' }, 1],
  ['interno', { commits: [{ sha: 'abc1234567', mensagem: 'ajuste ZZ_12' }] }, 1],
  ['interno', { descricao: 'abra em plataforma-ficticia' }, 1],
  ['interno', { descricao: 'https://app.plataforma-ficticia.example.invalid/x' }, 1],
  ['interno', { descricao: 'o termo-ficticio decidiu' }, 1],
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
  const esperados = [...config.regras.map((r) => r.id), 'interno'];
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

test('sem a lista só valem os genéricos e o aviso de checagem parcial aparece', () => {
  const achados = verificar({ branch: 'feat/zz-232-ajuste', descricao: 'abra em plataforma-ficticia' }, genericas);
  assert.deepEqual(achados, []);
  assert.match(formatar(achados, genericas), /::warning title=Bastidor::Checagem parcial.*BASTIDOR_NOMES_INTERNOS/);
  assert.ok(!formatar([], completas).includes('::warning'));
});

test('a variável inválida falha sem repetir o conteúdo', () => {
  assert.equal(lerInternos(undefined), null);
  assert.equal(lerInternos('   '), null);
  for (const bruto of ['nao e json segredo-ficticio', '[]', '{"outra":["segredo-ficticio"]}', '{"prefixosTarefa":"ZZ"}', '{"prefixosTarefa":["Z"]}', '{"prefixosTarefa":["ZZ"],"automacao":["a"]}']) {
    assert.throws(() => lerInternos(bruto), (e) => /inválida/.test(e.message) && !e.message.includes('segredo-ficticio'));
  }
});

test('a saída diz campo e linha, a categoria só das regras genéricas, e nunca repete o trecho', () => {
  const entrada = {
    titulo: 'ajuste',
    descricao: 'ok\nFeito com Claude Code\nrevisado pelo Codex e abra em plataforma-ficticia',
    branch: 'feat/zz-232-x',
    commits: [{ sha: 'ABCDEF1234567', mensagem: 'ajuste\n\nCo-Authored-By: Claude <noreply@anthropic.com>' }],
  };
  const saida = formatar(verificar(entrada, completas), completas);
  assert.match(saida, /\[autoria-ia\] campo descricao, linha 2 \(regra credito-autoria\)/);
  assert.match(saida, /\[autoria-ia\] campo descricao, linha 3 \(regra credito-revisao\)/);
  assert.match(saida, /texto interno encontrado: campo descricao, linha 3$/m);
  assert.match(saida, /texto interno encontrado: campo branch, linha 1$/m);
  assert.match(saida, /campo commit abcdef1, linha 3 \(regra trailer-ia\)/);
  const baixa = saida.toLowerCase();
  for (const proibido of ['claude', 'codex', 'anthropic', 'plataforma-ficticia', 'zz-232', 'termo-ficticio', 'noreply', '[plataforma]', '[tarefa]', 'tarefa-codigo', 'plataforma-nome', 'termo-interno']) {
    assert.ok(!baixa.includes(proibido), 'saída repetiu: ' + proibido);
  }
  assert.ok(saida.split('\n').every((l) => !l.startsWith('::') || /^::(?:error|warning) title=Bastidor::/.test(l)));
});

test('várias regras internas na mesma linha viram um só achado interno', () => {
  const achados = verificar({ descricao: 'ZZ-12 em plataforma-ficticia e termo-ficticio' }, completas);
  assert.deepEqual(achados, [{ categoria: 'interno', regra: 'interno', campo: 'descricao', linha: 1 }]);
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
    'Ｆｅｉｔｏ ｃｏｍ Ｃｌａｕｄｅ',
    'Revisão independente do Neon',
  ];
  for (const texto of evasoes) assert.ok(ids({ descricao: texto }).length > 0, texto);
  assert.equal(normalizar('Revisão ÁGUA'), 'revisao agua');
});

test('commits: o rótulo usa o início do hash ou a posição', () => {
  const achados = verificar({ commits: [{ sha: 'a'.repeat(40), mensagem: 'limpo' }, { sha: 'b'.repeat(40), mensagem: 'x\nfeito com Claude' }, { mensagem: 'revisado por Opus' }] }, completas);
  assert.deepEqual(achados.map((a) => a.campo + ':' + a.linha), ['commit bbbbbbb:2', 'commit 3:1']);
});

test('linhas longas e adversariais terminam rápido e a borda do pedaço não cria nem esconde achado', () => {
  const lixos = ['1,1,1,', 'https://a/', '<a ', 'feito com ', 'custo ', 'revisao independente ', 'gastou us$ 1,', 'co-authored-by: ', '1 maquinas ', 'o dono ', 'thread ', 'a '];
  const inicio = Date.now();
  for (const lixo of lixos) {
    const texto = lixo.repeat(Math.ceil(200000 / lixo.length));
    verificar({ titulo: texto, descricao: texto }, completas);
  }
  assert.ok(Date.now() - inicio < 5000, 'verificação lenta demais');
  assert.deepEqual(ids({ descricao: 'x'.repeat(1990) + ' feito com Claude ' + 'y'.repeat(3000) }), ['credito-autoria']);
  assert.deepEqual(ids({ descricao: 'x'.repeat(1700) + 'subagente ' + 'y'.repeat(400) }), [], 'começo do pedaço não satisfaz o limite de palavra');
  assert.deepEqual(ids({ descricao: 'a'.repeat(1990) + ' subagentez' + 'b'.repeat(500) }), [], 'fim do pedaço não satisfaz o limite de palavra');
  assert.deepEqual(ids({ descricao: 'a'.repeat(1990) + ' subagente ' + 'b'.repeat(500) }), ['subagente']);
});

test('texto enorme estoura o orçamento sem validar entrada truncada e a saída é limitada', () => {
  assert.throws(() => verificar({ descricao: 'a'.repeat(2_000_001) }, completas), (e) => e.controlado === true && /grande demais/.test(e.message));
  const muitos = verificar({ descricao: Array.from({ length: 500 }, () => 'feito com Claude').join('\n') }, completas);
  assert.equal(muitos.length, 200, 'para de acumular achados');
  const saida = formatar(muitos, completas).split('\n');
  assert.equal(saida.filter((l) => l.startsWith('::error')).length, 51);
  assert.ok(saida.some((l) => /há outros achados/.test(l)));
});

function resposta(status, corpo) {
  return { ok: status >= 200 && status < 300, status, json: async () => corpo };
}

const SHA = 'a'.repeat(40);
const prDe = (extra = {}) => ({ title: 'ajuste', body: 'texto limpo', head: { ref: 'feat/ajuste', sha: SHA }, commits: 1, ...extra });

// pr: um estado (devolvido sempre) ou uma lista de estados (um por leitura do PR; o último se repete).
function apiFalsa({ pr, commits = [] }) {
  const chamadas = [];
  let leituras = 0;
  const buscar = async (url, opcoes = {}) => {
    const alvo = new URL(url);
    const rota = (opcoes.method ?? 'GET') + ' ' + alvo.pathname;
    chamadas.push(rota);
    assert.ok(opcoes.signal, 'toda chamada tem prazo');
    if (rota === 'GET /repos/o/r/pulls/7') {
      const estados = Array.isArray(pr) ? pr : [pr];
      return resposta(200, estados[Math.min(leituras++, estados.length - 1)]);
    }
    if (rota === 'GET /repos/o/r/pulls/7/commits') {
      const pagina = Number(alvo.searchParams.get('page'));
      return resposta(200, commits.slice((pagina - 1) * 100, pagina * 100));
    }
    return resposta(404, {});
  };
  return { buscar, chamadas };
}

const AMBIENTE = { GITHUB_REPOSITORY: 'o/r', GITHUB_API_URL: 'https://api.example.invalid', GH_TOKEN: 'token-ficticio', PR_NUMBER: '7', PR_HEAD_SHA: SHA, BASTIDOR_FORK: 'false', BASTIDOR_NOMES_INTERNOS: brutoInternos };
const commit = (i, mensagem = 'ajuste ' + i) => ({ sha: String(i).padStart(40, '0'), commit: { message: mensagem } });

async function capturar(fn) {
  const original = console.log;
  const linhas = [];
  console.log = (...args) => linhas.push(args.join(' '));
  try { return { codigo: await fn(), saida: linhas.join('\n') }; } finally { console.log = original; }
}

const rodar = (env, api) => capturar(() => main(['pr', '--regras', arquivoRegras], env, api.buscar));

test('modo pr lê título, descrição, branch e todas as páginas de commits pela API', async () => {
  const commits = Array.from({ length: 101 }, (_, i) => commit(i + 1));
  commits[100] = commit(101, 'x\nfeito com Claude');
  const api = apiFalsa({ pr: prDe({ commits: 101 }), commits });
  const entrada = await lerPr(AMBIENTE, api.buscar);
  assert.equal(entrada.commits.length, 101);
  assert.equal(api.chamadas.filter((c) => c.endsWith('/commits')).length, 2);
  const { codigo, saida } = await rodar(AMBIENTE, api);
  assert.equal(codigo, 1);
  assert.match(saida, /campo commit 0000000, linha 2 \(regra credito-autoria\)/);
  assert.ok(!saida.toLowerCase().includes('claude'));
});

test('modo pr passa limpo, reprova título e branch e falha fechado quando a API não lista tudo', async () => {
  const limpo = apiFalsa({ pr: prDe({ title: 'feat(host): conectar o Codex', body: null, head: { ref: 'feat/codex', sha: SHA } }), commits: [commit(1)] });
  assert.equal((await rodar(AMBIENTE, limpo)).codigo, 0);
  const sujo = apiFalsa({ pr: prDe({ title: 'Feito com Claude', head: { ref: 'feat/zz-1-x', sha: SHA } }), commits: [commit(1)] });
  const { codigo, saida } = await rodar(AMBIENTE, sujo);
  assert.equal(codigo, 1);
  assert.match(saida, /campo titulo, linha 1/);
  assert.match(saida, /texto interno encontrado: campo branch, linha 1/);
  const incompleto = apiFalsa({ pr: prDe({ commits: 400 }), commits: [commit(1)] });
  await assert.rejects(lerPr(AMBIENTE, incompleto.buscar), /mais commits/);
  await assert.rejects(lerPr({ ...AMBIENTE, PR_NUMBER: '7; rm' }, limpo.buscar), /PR_NUMBER/);
  await assert.rejects(lerPr({ ...AMBIENTE, PR_HEAD_SHA: 'xyz' }, limpo.buscar), /PR_HEAD_SHA/);
  await assert.rejects(lerPr({ ...AMBIENTE, GITHUB_REPOSITORY: '../x' }, limpo.buscar), /GITHUB_REPOSITORY/);
  await assert.rejects(lerPr(AMBIENTE, apiFalsa({ pr: { body: '', head: {} } }).buscar), /sem os campos do PR/);
  await assert.rejects(lerPr(AMBIENTE, apiFalsa({ pr: prDe(), commits: [{ sha: 'c'.repeat(40) }] }).buscar), /sem as mensagens/);
});

test('a fotografia do PR é conferida: SHA do evento no início e tudo de novo no fim', async () => {
  const commits = [commit(1)];
  await assert.rejects(lerPr(AMBIENTE, apiFalsa({ pr: prDe({ head: { ref: 'feat/ajuste', sha: 'b'.repeat(40) } }), commits }).buscar), /mudou durante a verificação/);
  for (const depois of [{ title: 'outro título' }, { body: 'outra descrição' }, { head: { ref: 'feat/outro', sha: SHA } }, { head: { ref: 'feat/ajuste', sha: 'b'.repeat(40) } }, { commits: 2 }]) {
    const api = apiFalsa({ pr: [prDe(), prDe(depois)], commits });
    await assert.rejects(lerPr(AMBIENTE, api.buscar), /mudou durante a verificação/, JSON.stringify(Object.keys(depois)));
  }
  const estavel = apiFalsa({ pr: [prDe(), prDe()], commits });
  assert.equal((await lerPr(AMBIENTE, estavel.buscar)).titulo, 'ajuste');
  assert.equal(estavel.chamadas.filter((c) => c === 'GET /repos/o/r/pulls/7').length, 2);
});

test('em PR do próprio repositório a lista interna é obrigatória e a falha não repete o conteúdo', async () => {
  const api = apiFalsa({ pr: prDe(), commits: [commit(1)] });
  for (const valor of [undefined, '', '   ', '{}', '{"prefixosTarefa":[],"plataformas":[]}', '{"prefixosTarefa":["ZZ"]}', 'nao-json-ficticio']) {
    const { BASTIDOR_NOMES_INTERNOS, ...semLista } = AMBIENTE;
    const env = valor === undefined ? semLista : { ...semLista, BASTIDOR_NOMES_INTERNOS: valor };
    await assert.rejects(main(['pr', '--regras', arquivoRegras], env, api.buscar), (e) => e.controlado === true && /BASTIDOR_NOMES_INTERNOS/.test(e.message) && !e.message.includes('nao-json-ficticio'));
  }
  assert.deepEqual(api.chamadas, [], 'sem lista, nenhuma chamada à API');
  assert.equal(exigirInternos('{"prefixosTarefa":["ZZ"],"plataformas":["xx-ficticia"]}').prefixosTarefa.length, 1);
  await assert.rejects(main(['pr', '--regras', arquivoRegras], { ...AMBIENTE, BASTIDOR_FORK: undefined }, api.buscar), /BASTIDOR_FORK/);
  await assert.rejects(main(['pr', '--regras', arquivoRegras], { ...AMBIENTE, BASTIDOR_FORK: 'talvez' }, api.buscar), /BASTIDOR_FORK/);
});

test('PR de fork roda só com as regras genéricas, sem a lista, e não revela nada da lista', async () => {
  const { BASTIDOR_NOMES_INTERNOS, ...semLista } = AMBIENTE;
  const interno = apiFalsa({ pr: prDe({ title: 'abra em plataforma-ficticia', head: { ref: 'feat/zz-1-x', sha: SHA } }), commits: [commit(1)] });
  const fork = await rodar({ ...semLista, BASTIDOR_FORK: 'true' }, interno);
  assert.equal(fork.codigo, 0, 'termo interno não é verificado em fork');
  assert.match(fork.saida, /PR de fork: só as regras genéricas/);
  assert.ok(!fork.saida.includes('::warning') && !fork.saida.includes('add-mask'));
  const comLista = await rodar({ ...AMBIENTE, BASTIDOR_FORK: 'true' }, apiFalsa({ pr: prDe({ title: 'abra em plataforma-ficticia' }), commits: [commit(1)] }));
  assert.equal(comLista.codigo, 0, 'mesmo que a lista chegasse, o fork não a usa');
  const generico = await rodar({ ...semLista, BASTIDOR_FORK: 'true' }, apiFalsa({ pr: prDe({ title: 'Feito com Claude' }), commits: [commit(1)] }));
  assert.equal(generico.codigo, 1);
  assert.match(generico.saida, /\[autoria-ia\] campo titulo, linha 1/);
});

test('a lista interna sai mascarada no log, só em PR do próprio repositório', async () => {
  const linhas = mascaras(internos);
  assert.ok(linhas.includes('::add-mask::plataforma-ficticia') && linhas.includes('::add-mask::termo-ficticio'));
  assert.ok(linhas.every((l) => /^::add-mask::[A-Za-z0-9. -]+$/.test(l)));
  const { saida } = await rodar(AMBIENTE, apiFalsa({ pr: prDe(), commits: [commit(1)] }));
  assert.ok(saida.split('\n')[0].startsWith('::add-mask::'));
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
    const rodarLocal = (...extra) => spawnSync(process.execPath, [resolve('tools/ci-bastidor.mjs'), 'local', '--regras', arquivoRegras, '--base', base, ...extra], { cwd: pasta, encoding: 'utf8', env });
    const parcial = rodarLocal('--titulo', 'feat: conectar o Codex');
    assert.equal(parcial.status, 0);
    assert.match(parcial.stdout, /Checagem parcial/);
    const ruim = rodarLocal('--titulo', 'Feito com Claude');
    assert.equal(ruim.status, 1);
    assert.match(ruim.stdout, /campo titulo, linha 1/);
    assert.ok(!ruim.stdout.toLowerCase().includes('claude'));
    writeFileSync(join(pasta, 'c.txt'), 'c'); git('add', '.'); git('-c', 'core.hooksPath=/dev/null', 'commit', '-qm', 'outro\n\nRevisado por Opus');
    const commitRuim = rodarLocal();
    assert.equal(commitRuim.status, 1);
    assert.match(commitRuim.stdout, /campo commit [0-9a-f]{7}, linha 3 \(regra credito-revisao\)/);
    assert.equal(rodarLocal('--base', 'ref-que-nao-existe').status, 2);
    const completo = spawnSync(process.execPath, [resolve('tools/ci-bastidor.mjs'), 'local', '--regras', arquivoRegras, '--base', base], { cwd: pasta, encoding: 'utf8', env: { ...env, BASTIDOR_NOMES_INTERNOS: brutoInternos } });
    assert.ok(!completo.stdout.includes('Checagem parcial') && !completo.stdout.includes('add-mask'));
  } finally { rmSync(pasta, { recursive: true, force: true }); }
});

const bastidor = readFileSync(new URL('../.github/workflows/bastidor.yml', import.meta.url), 'utf8');

test('workflow: só ASCII, permissões de leitura, ação fixada por SHA, sem checkout e sem código do PR', () => {
  assert.ok(!/[^\x00-\x7F]/.test(bastidor), 'deve ser ASCII');
  assert.match(bastidor, /^permissions: \{\}$/m);
  for (const proibido of ['workflow_run', 'schedule', 'push:', 'workflow_dispatch', 'merge_group', 'actions/checkout', 'container:', 'services:', 'issue_comment', 'pull_request_review']) {
    assert.ok(!bastidor.includes(proibido), proibido);
  }
  assert.deepEqual([...bastidor.matchAll(/^\s+(?:- )?uses: (\S+)/gm)].map((m) => m[1]), ['actions/setup-node@949feb2413d6458794dcd2491c4babbbce0c15c1']);
  assert.deepEqual([...bastidor.matchAll(/^      ([a-z-]+): (read|write)$/gm)].map((m) => m[1] + ':' + m[2]), ['contents:read', 'pull-requests:read']);
  assert.ok(!/: write\b/.test(bastidor));
  assert.match(bastidor, /^    timeout-minutes: [1-9][0-9]*$/m);
  assert.match(bastidor, /if: \$\{\{ runner\.environment == 'github-hosted' && github\.event\.repository\.private \}\}/);
  assert.ok(bastidor.includes('Accept: application/vnd.github.sha') && bastidor.includes('--data-urlencode "ref=$sha"'), 'verificador e regras do mesmo commit');
  assert.ok(!bastidor.includes('404'), 'sem caminho alternativo para o código do PR');
  assert.ok(!/github\.(head_ref|event\.pull_request\.head\.ref)/.test(bastidor), 'nunca usa a ref do PR');
});

test('workflow: máquina vinda de variável, concorrência por PR e pull_request_target justificado', () => {
  assert.match(bastidor, /^on:\n  pull_request_target:\n    types: \[opened, edited, reopened, synchronize\]\n\n/m);
  assert.match(bastidor, /^# Unico workflow do repositorio em pull_request_target, e so por isto:/m);
  assert.match(bastidor, /group: bastidor-\$\{\{ github\.event\.pull_request\.number \}\}\n  cancel-in-progress: true/);
  assert.equal((bastidor.match(/^    runs-on: (.+)$/gm) ?? []).length, 1);
  assert.match(bastidor, /^    environment: bastidor$/m, 'segredo de ambiente, restrito à branch protegida');
  assert.ok(bastidor.includes("    runs-on: ${{ vars.CI_MAQUINA_LEVE || 'instabox-small' }}"));
  assert.ok(!/ubuntu-|windows-|macos-|self-hosted|boat|nscloud/i.test(bastidor), 'rótulo de máquina fora do padrão');
  assert.deepEqual([...new Set([...bastidor.matchAll(/\bvars\.(\w+)/g)].map((m) => m[1]))], ['CI_MAQUINA_LEVE']);
  assert.match(bastidor, /enquanto ela apontar para maquina paga, o repositorio nao pode ser publico/);
});

test('workflow: o segredo só chega ao verificador em PR do próprio repositório, nunca em fork, e nunca ao shell', () => {
  assert.deepEqual([...new Set([...bastidor.matchAll(/\bsecrets\.(\w+)/g)].map((m) => m[1]))], ['BASTIDOR_NOMES_INTERNOS']);
  assert.equal((bastidor.match(/secrets\./g) ?? []).length, 1);
  assert.ok(bastidor.includes("BASTIDOR_NOMES_INTERNOS: ${{ github.event.pull_request.head.repo.full_name == github.repository && secrets.BASTIDOR_NOMES_INTERNOS || '' }}"));
  assert.ok(bastidor.includes('BASTIDOR_FORK: ${{ github.event.pull_request.head.repo.full_name != github.repository }}'));
  assert.match(bastidor, /PR de fork roda so com as\n# regras genericas e nunca recebe o segredo/);
});

test('workflow: texto de PR nunca entra em comando nem em expressão de execução', () => {
  const linhas = bastidor.split('\n');
  linhas.forEach((linha, i) => {
    const bloco = /^(\s+)run: (.*)$/.exec(linha);
    if (!bloco) return;
    const corpo = [bloco[2]];
    if (bloco[2].trim() === '|') {
      for (let j = i + 1; j < linhas.length && (linhas[j].trim() === '' || linhas[j].startsWith(bloco[1] + ' '));) corpo.push(linhas[j++]);
    }
    assert.ok(!corpo.join('\n').includes('${{'), 'expressão dentro de run na linha ' + (i + 1));
  });
  assert.ok(!/github\.(head_ref|event\.(pull_request\.(title|body|head\.ref)|comment\.body|review\.body|issue\.(title|body)))/.test(bastidor));
  assert.ok(bastidor.includes('node "$RUNNER_TEMP/bastidor/ci-bastidor.mjs" pr'));
});

test('o contrato do workflow e o teste do verificador rodam no CI de ferramentas', () => {
  const ci = readFileSync(new URL('../.github/workflows/ci.yml', import.meta.url), 'utf8');
  assert.ok(ci.includes('tools/ci-bastidor.test.mjs'));
});
