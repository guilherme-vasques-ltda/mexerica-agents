// Propaga a versao da release aos metadados do pacote do app e confere o que foi empacotado.
// Node puro, sem dependencias. No modo release, o semver sem o v e gravado em
// apps/desktop/package.json na VM descartavel, antes de empacotar, para app.getVersion(),
// Info.plist, .deb, AppImage e NSIS reportarem a versao da rodada; no Windows, o package.json
// de dentro do app.asar e conferido no fim. No preview, os metadados mantem a versao base do
// package.json: preview-<commit> nao e versao valida para .deb nem NSIS. Guia: docs/guides/ci.md.
// Uso:
//   node tools/ci-release-version.mjs --package apps/desktop/package.json --version v0.0.1
//   node tools/ci-release-version.mjs --check-asar dist/win-unpacked/resources/app.asar --version v0.0.1
import { readFileSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { validarVersao } from './ci-release-notes.mjs';

// Só mensagens controladas chegam ao log: erro de leitura pode trazer pedaço do que lia.
const falha = (mensagem) => Object.assign(new Error(mensagem), { controlado: true });

export function versaoSemver(versao) {
  if (!validarVersao(versao)) throw falha('Versão inválida: use o formato v0.0.1, com sufixo opcional em minúsculas');
  return versao.slice(1);
}

export function aplicarVersaoPacote(caminhoPackageJson, versao) {
  const semver = versaoSemver(versao);
  let pkg;
  try { pkg = JSON.parse(readFileSync(caminhoPackageJson, 'utf8')); } catch { throw falha('package.json ilegível'); }
  if (!pkg || typeof pkg !== 'object' || Array.isArray(pkg) || typeof pkg.version !== 'string') throw falha('package.json sem campo version');
  pkg.version = semver;
  writeFileSync(caminhoPackageJson, `${JSON.stringify(pkg, null, 2)}\n`);
  // Confirma na releitura: sem a versão certa gravada, a rodada reprova em vez de seguir.
  if (JSON.parse(readFileSync(caminhoPackageJson, 'utf8')).version !== semver) throw falha('A versão não foi gravada de fato no package.json');
  return semver;
}

// Cabecalho do asar: [0-3]=4, [4-7]=tamanho do bloco de cabecalho, [8-11]=tamanho do payload,
// [12-15]=tamanho do JSON, [16..]=JSON; os arquivos comecam em 8 + bloco e cada offset e
// relativo a essa base. O package.json de dentro do pacote e a prova do que o app reporta.
export function conferirVersaoAsar(caminhoAsar, versao) {
  const semver = versaoSemver(versao);
  let dados;
  try { dados = readFileSync(caminhoAsar); } catch { throw falha('app.asar ilegível'); }
  try {
    if (dados.length < 16) throw falha('app.asar com cabeçalho menor que o mínimo');
    const bloco = dados.readUInt32LE(4);
    const payload = dados.readUInt32LE(8);
    const tamanhoJson = dados.readUInt32LE(12);
    if (payload < 4 || bloco < payload + 4 || tamanhoJson > payload - 4 || dados.length < 8 + bloco || dados.length < 16 + tamanhoJson) {
      throw falha('app.asar com cabeçalho fora dos limites do arquivo');
    }
    const cabecalho = JSON.parse(dados.subarray(16, 16 + tamanhoJson).toString('utf8'));
    const arquivo = cabecalho?.files?.['package.json'];
    if (!arquivo || arquivo.size == null || arquivo.offset == null) throw falha('app.asar sem package.json no cabeçalho');
    const inicio = 8 + bloco + Number(arquivo.offset);
    if (!Number.isSafeInteger(inicio) || !Number.isSafeInteger(arquivo.size) || inicio < 8 + bloco || arquivo.size < 0 || dados.length < inicio + arquivo.size) {
      throw falha('package.json fora dos limites do app.asar');
    }
    const pkg = JSON.parse(dados.subarray(inicio, inicio + arquivo.size).toString('utf8'));
    if (pkg?.version !== semver) throw falha(`O package.json de dentro do app.asar não tem a versão ${semver}`);
    return semver;
  } catch (erro) {
    throw erro.controlado ? erro : falha('app.asar com cabeçalho ilegível');
  }
}

function lerArgumentos(argv) {
  const opcoes = {};
  for (let i = 0; i < argv.length; i++) {
    if (!argv[i].startsWith('--') || i + 1 >= argv.length) throw falha('Argumento inválido');
    opcoes[argv[i].slice(2)] = argv[++i];
  }
  return opcoes;
}

export function main(argv = process.argv.slice(2)) {
  const opcoes = lerArgumentos(argv);
  // lerArgumentos guarda as chaves com hífen: --check-asar e só as opções conhecidas passam.
  const permitidas = new Set(['version', 'package', 'check-asar']);
  for (const chave of Object.keys(opcoes)) if (!permitidas.has(chave)) throw falha(`Argumento --${chave} desconhecido`);
  if (!opcoes.version) throw falha('Argumento --version obrigatório');
  const packageJson = opcoes.package;
  const checkAsar = opcoes['check-asar'];
  if (packageJson && checkAsar) throw falha('Escolha --package ou --check-asar, não os dois');
  if (packageJson) return aplicarVersaoPacote(packageJson, opcoes.version);
  if (checkAsar) return conferirVersaoAsar(checkAsar, opcoes.version);
  throw falha('Argumento --package ou --check-asar obrigatório');
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    main();
  } catch (erro) {
    console.log(`::error title=Versao da release::${erro.controlado ? erro.message : 'Falha inesperada na versão da release'}`);
    process.exitCode = 1;
  }
}
