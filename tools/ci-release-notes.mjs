// Notas de lancamento para o rascunho da release multiplataforma. Node puro, sem dependencias.
// As mudancas vem do resumo automatico da API do GitHub, passam pela mesma trava contra bastidor
// de tools/ci-bastidor.mjs (regras de .github/security/bastidor.json) e o texto final e
// re-verificado linha a linha antes de sair: achado qualquer reprova, nada e publicado.
// Uso: node tools/ci-release-notes.mjs --version v0.0.1 --sha <sha> --out arquivo.md
import { readFileSync, writeFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { compilarRegras, lerInternos, verificar } from './ci-bastidor.mjs';

const REGRAS_PADRAO = resolve(dirname(fileURLToPath(import.meta.url)), '../.github/security/bastidor.json');
const PRAZO_API_MS = 30_000;
// Só mensagens controladas chegam ao log: erro de parse ou de rede pode trazer pedaço do que lia.
const falha = (mensagem) => Object.assign(new Error(mensagem), { controlado: true });

// SemVer estrito: sem zeros a esquerda no core e nos identificadores puramente numericos, e no maximo 64 caracteres.
const VERSAO_ER = /^v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(-((0|[1-9][0-9]*)|[0-9]*[a-z][a-z0-9]*)(\.((0|[1-9][0-9]*)|[0-9]*[a-z][a-z0-9]*))*)?$/;

export function validarVersao(versao) {
  if (typeof versao !== 'string') return false;
  if (versao.length > 64) return false;
  if (!VERSAO_ER.test(versao)) return false;
  if (versao.includes('..') || versao.includes('-.') || versao.endsWith('.')) return false;
  return true;
}

// Resumo automatico da API do GitHub. Falha controlada, sem imprimir token, corpo ou URL com credencial.
export async function gerarNotasGithub({ repo, version, sha, token, apiUrl }, buscar = fetch) {
  if (!validarVersao(version)) throw falha('Versao invalida');
  if (!/^[0-9a-f]{40}$/.test(sha ?? '')) throw falha('SHA invalido');
  if (!/^(?!\.{1,2}\/)[\w.-]+\/(?!\.{1,2}$)[\w.-]+$/.test(repo ?? '')) throw falha('Repositorio invalido');
  let resposta;
  try {
    resposta = await buscar(`${apiUrl ?? 'https://api.github.com'}/repos/${repo}/releases/generate-notes`, {
      method: 'POST',
      headers: {
        Authorization: `Bearer ${token}`,
        Accept: 'application/vnd.github+json',
        'X-GitHub-Api-Version': '2022-11-28',
        'Content-Type': 'application/json',
      },
      body: JSON.stringify({ tag_name: version, target_commitish: sha }),
      signal: AbortSignal.timeout(PRAZO_API_MS),
    });
  } catch {
    throw falha('Nao foi possivel pedir as notas a API do GitHub');
  }
  if (resposta.status !== 200) throw falha(`A API do GitHub nao gerou as notas (HTTP ${resposta.status})`);
  let dados;
  try { dados = await resposta.json(); } catch { throw falha('Resposta da API sem corpo legivel'); }
  if (typeof dados?.body !== 'string') throw falha('Resposta da API sem o corpo das notas');
  return dados.body;
}

// Linha a linha pela trava de bastidor: linha com achado sai sem aparecer em log nenhum.
// O resumo da API abre com o titulo de secao dele; a nota ja tem o cabecalho das mudancas,
// então a linha sai sem contar como achado. Cabecalhos ## restantes viram #### para nao
// quebrar a hierarquia da nota.
export function filtrarChangelog(bruto, regras) {
  const mantidas = [];
  let omitidas = 0;
  for (const linha of String(bruto ?? '').split(/\r\n|\r|\n/)) {
    const limpa = linha.trim();
    if (!limpa) continue;
    if (/^##\s+What's Changed$/i.test(limpa)) continue;
    if (verificar({ descricao: linha }, regras).length) {
      omitidas++;
      continue;
    }
    mantidas.push(/^##\s+/.test(limpa) ? limpa.replace(/^##\s+/, '#### ') : limpa);
  }
  return { linhas: mantidas, omitidas };
}

const CHANGELOG_PADRAO = 'Primeira versão de pré-lançamento publicada a partir da branch main.';

export function montarNotas({ version, sha, changelog, regras }) {
  if (!validarVersao(version)) throw falha('Versao invalida');
  if (!/^[0-9a-f]{40}$/.test(sha ?? '')) throw falha('SHA invalido');
  const brutas = Array.isArray(changelog)
    ? changelog.map((linha) => String(linha).trim())
    : String(changelog ?? '').split(/\r\n|\r|\n/).map((linha) => linha.trim());
  const linhas = brutas.filter(Boolean);
  const mudancas = linhas.length ? linhas.join('\n') : CHANGELOG_PADRAO;
  const corpo = [
    `## Mexerica Open ${version}`,
    '',
    'Pré-lançamento gerado a partir da branch `main`. Este rascunho ainda não é a versão oficial.',
    '',
    '### Aviso importante (pré-lançamento sem assinatura)',
    '',
    'Esta versão ainda não tem assinatura Apple Developer (macOS) nem Authenticode (Windows).',
    'Ao abrir pela primeira vez, o Gatekeeper (macOS) e o SmartScreen (Windows) mostram um aviso',
    'de segurança; confira o hash dos arquivos antes de abrir. Os pacotes usam o perfil padrão',
    'do app e podem compartilhar dados com instalações existentes.',
    '',
    '### Arquivos incluídos',
    '',
    `- \`Mexerica-Open-${version}-macos-arm64.zip\` — macOS Apple Silicon (app + Hub local), assinatura ad hoc, perfil padrão do app, não isolado de uma instalação existente.`,
    `- \`mexerica-open-${version}-linux-amd64.deb\` — Linux 64 bits, cliente desktop do Hub via SSH, perfil padrão do app (\`Donnabot\` / \`.mexerica\`), não isolado de uma instalação existente.`,
    `- \`mexerica-open-${version}-linux-x86_64.AppImage\` — Linux 64 bits portátil, cliente desktop do Hub via SSH, perfil padrão do app (\`Donnabot\` / \`.mexerica\`), não isolado de uma instalação existente.`,
    `- \`mexerica-open-${version}-windows-x64-setup.exe\` — Windows 64 bits (app + Hub local), instalador por usuário, perfil padrão do app (\`Donnabot\` / \`.mexerica\`, porta \`19422\`), não isolado de uma instalação existente.`,
    `- \`mexerica-open-${version}-windows-x64-portable.zip\` — versão portátil Windows 64 bits, perfil padrão do app (\`Donnabot\` / \`.mexerica\`, porta \`19422\`), não isolado de uma instalação existente.`,
    '- `SHA256SUMS.txt` — hashes SHA-256 de todos os arquivos.',
    '',
    '### Como instalar e abrir',
    '',
    '- **macOS (perfil padrão):** descompacte o zip e abra o `Mexerica.app`. Não substitua uma',
    '  instalação existente sem querer: mesmo em outra pasta, o app pode compartilhar dados.',
    '  Na primeira abertura, use botão direito → Abrir, ou Ajustes → Privacidade e Segurança → Abrir Mesmo Assim.',
    `- **Linux (perfil padrão):** instale com \`sudo apt install ./mexerica-open-${version}-linux-amd64.deb\`, ou dê`,
    '  permissão de execução ao `.AppImage` (`chmod +x`). Usa o perfil padrão do app: substitui o pacote',
    '  instalado ou compartilha dados com uma instalação existente na mesma máquina.',
    `- **Windows (perfil padrão):** execute o \`setup.exe\` para instalar por usuário, ou extraia o \`portable.zip\``,
    '  e abra o `Mexerica.exe`. Usa o perfil padrão do app: mesmo a versão portátil compartilha dados e',
    '  preferências, e o serviço local do Hub pode apontar para o executável deste pacote se já houver',
    '  outra instalação na máquina. No aviso do SmartScreen, use Mais informações → Executar assim mesmo.',
    '',
    '### Conferência de integridade (SHA-256)',
    '',
    'Cada comando confere só o arquivo baixado e falha se o arquivo não estiver na pasta, não constar',
    'no `SHA256SUMS.txt` ou tiver hash diferente. Rode na pasta onde estão o arquivo baixado e o',
    '`SHA256SUMS.txt`:',
    '',
    '- macOS:',
    '  ```sh',
    `  awk -v f="Mexerica-Open-${version}-macos-arm64.zip" '$2 == f' SHA256SUMS.txt | shasum -a 256 -c -`,
    '  ```',
    '- Linux (troque o nome pelo `.AppImage` se tiver baixado ele):',
    '  ```sh',
    `  awk -v f="mexerica-open-${version}-linux-amd64.deb" '$2 == f' SHA256SUMS.txt | sha256sum -c -`,
    '  ```',
    '- Windows (PowerShell; troque `$f` pelo `portable.zip` se tiver baixado a versão portátil):',
    '  ```powershell',
    `  $f = "mexerica-open-${version}-windows-x64-setup.exe"; $linhas = @(Select-String -Path SHA256SUMS.txt -SimpleMatch "  $f" | Where-Object { $_.Line.EndsWith("  $f") }); if ($linhas.Count -ne 1) { throw "Arquivo nao encontrado ou repetido no SHA256SUMS.txt" }; $esperado = ($linhas[0].Line -split '\\s+')[0].ToUpper(); $atual = (Get-FileHash $f -Algorithm SHA256).Hash.ToUpper(); if ($atual -ne $esperado) { throw "Hash divergente" }; "OK: $f"`,
    '  ```',
    '',
    '### Mudanças nesta versão',
    '',
    mudancas,
    '',
    `Commit: ${sha}`,
    '',
  ].join('\n');
  // Verificacao final fail-closed: qualquer achado no proprio texto reprova antes de escrever o arquivo.
  if (verificar({ titulo: `Mexerica Open ${version}`, descricao: corpo }, regras).length) {
    throw falha('O texto das notas foi reprovado pela verificacao de bastidor; nada foi escrito');
  }
  return corpo;
}

function lerArgumentos(argv) {
  const opcoes = {};
  for (let i = 0; i < argv.length; i++) {
    if (!argv[i].startsWith('--') || i + 1 >= argv.length) throw falha('Argumento inválido');
    opcoes[argv[i].slice(2)] = argv[++i];
  }
  return opcoes;
}

export async function main(argv = process.argv.slice(2), env = process.env, buscar = fetch) {
  const opcoes = lerArgumentos(argv);
  const versao = opcoes.version;
  const sha = opcoes.sha;
  if (!validarVersao(versao)) throw falha('Versão inválida: use o formato v0.0.1, com sufixo opcional em minúsculas');
  if (!/^[0-9a-f]{40}$/.test(sha ?? '')) throw falha('SHA inválido');
  if (!opcoes.out) throw falha('Argumento --out obrigatório');
  const regras = compilarRegras(JSON.parse(readFileSync(REGRAS_PADRAO, 'utf8')), lerInternos(env.BASTIDOR_NOMES_INTERNOS));
  const bruto = await gerarNotasGithub({
    repo: env.GITHUB_REPOSITORY,
    version: versao,
    sha,
    token: env.GH_TOKEN,
    apiUrl: env.GITHUB_API_URL,
  }, buscar);
  const { linhas, omitidas } = filtrarChangelog(bruto, regras);
  if (omitidas > 0) console.log(`::warning::${omitidas} linha(s) omitida(s) do resumo automatico pela verificacao de bastidor.`);
  const notas = montarNotas({ version: versao, sha, changelog: linhas, regras });
  writeFileSync(opcoes.out, notas);
  return 0;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main().then(() => { process.exitCode = 0; }, (erro) => {
    console.log(`::error title=Notas de release::${erro.controlado ? erro.message : 'Falha inesperada ao gerar as notas'}`);
    process.exitCode = 1;
  });
}
