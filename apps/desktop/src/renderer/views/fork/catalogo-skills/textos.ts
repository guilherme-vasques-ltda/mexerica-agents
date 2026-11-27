// Textos da biblioteca de skills (D166/D167). D454 (fatia 8b): vêm do catálogo `skills.*` nos 6 idiomas.
// Cada texto é uma `Frase`: a chave e os parâmetros, traduzidos só ao mostrar. A loja e as regras guardam a frase,
// nunca o texto pronto (lição da R1 do #572): trocar o idioma com a tela aberta muda também falhas e avisos.
import { cru, ehFrase, frase, juntas, plural, textoDe, ErroFrase, type Frase, type SufixoSemParams, type Valor } from '../../../lib/fork-i18n/frase.ts'
import type { AppLanguage } from '../../../../shared/fork-idioma.ts'

export { cru, ehFrase, frase, juntas, plural, textoDe, ErroFrase, type Frase, type SufixoSemParams }

const f = frase
const R = 'skills.revisao.'
type CodigoFalha = SufixoSemParams<`${typeof R}falha.`> extends `${infer C}.titulo` | `${string}.detalhe` ? C : never
const falha = (codigo: CodigoFalha): [Frase, Frase] => [f(`${R}falha.${codigo}.titulo`), f(`${R}falha.${codigo}.detalhe`)]

export const frases = {
  ondeLigar: f('skills.ondeLigar'),
  instalarSoTexto: f('skills.instalarSoTexto'),
  soOrienta: f('skills.soOrienta'),
  semPreparar: f('skills.semPreparar'),
  reciboNaoEAuditoria: f('skills.reciboNaoEAuditoria'),
  semRecibo: f('skills.semRecibo'),
  cobertura: { pinnedPassiveTextOnly: f('skills.cobertura.pinnedPassiveTextOnly') } as Record<string, Frase>,
  semConexao: f('skills.semConexao'),
  semConexaoDetalhe: (hub: string) => f('skills.semConexaoDetalhe', { hub }),
  semAcesso: f('skills.semAcesso'),
  semAcessoDetalhe: (hub: string) => f('skills.semAcessoDetalhe', { hub }),
  hubAntigo: f('skills.hubAntigo'),
  hubAntigoDetalhe: f('skills.hubAntigoDetalhe'),
  // Hub sem o protocolo forte (resposta sem `skillProtocol: 2`, inclusive lista vazia).
  semProtocolo: f('skills.semProtocolo'),
  semProtocoloDetalhe: f('skills.semProtocoloDetalhe'),
  // Recusas comprovadas antes do efeito.
  recusaProtocolo: f('skills.recusaProtocolo'),
  instalacaoMudou: f('skills.instalacaoMudou'),
  // Conteúdo que não confere ou não pode ser lido no Hub (`contentStatus`).
  conteudo: { changed: f('skills.conteudo.changed'), unavailable: f('skills.conteudo.unavailable') },
  conteudoMotivo: { changed: f('skills.conteudoMotivo.changed'), unavailable: f('skills.conteudoMotivo.unavailable') },
  conteudoInstrucoes: { changed: f('skills.conteudoInstrucoes.changed'), unavailable: f('skills.conteudoInstrucoes.unavailable') },
  // D306: o que o dono pode fazer com uma skill bloqueada. Nada é restaurado sozinho.
  recuperar: { curada: f('skills.recuperar.curada'), manual: f('skills.recuperar.manual') },
  reinstalada: (nome: string) => f('skills.reinstalada', { nome }),
  // Resultado incerto (`skill_mutation_outcome_unknown`): enviado, sem resposta confiável. Nunca "nada ocorreu".
  incerto: {
    instalar: (nome: string) => f('skills.incerto.instalar', { nome }),
    remover: (nome: string) => f('skills.incerto.remover', { nome }),
    reinstalar: (nome: string) => f('skills.incerto.reinstalar', { nome }),
    escrever: (nome: string) => f('skills.incerto.escrever', { nome }),
  },
  incertoAgora: {
    instalada: (nome: string) => f('skills.incertoAgora.instalada', { nome }),
    naoInstalada: (nome: string) => f('skills.incertoAgora.naoInstalada', { nome }),
    ausente: (nome: string) => f('skills.incertoAgora.ausente', { nome }),
    presente: (nome: string) => f('skills.incertoAgora.presente', { nome }),
    conferida: (nome: string) => f('skills.incertoAgora.conferida', { nome }),
    naoConferida: (nome: string) => f('skills.incertoAgora.naoConferida', { nome }),
  },
  incertoConferir: f('skills.incertoConferir'),
  incertoSemRemover: f('skills.incertoSemRemover'),
  incertoEscreverDeNovo: f('skills.incertoEscreverDeNovo'),
  incertoEscreverAnterior: (nome: string) => f('skills.incertoEscreverAnterior', { nome }),
  // Folha da skill nos ajustes de bot (D166 R10): fora de uma leitura válida, nada da skill aparece.
  folhaTitulo: f('skills.folhaTitulo'),
  folhaOutroHub: f('skills.folhaOutroHub'),
  folhaConexaoMudou: f('skills.folhaConexaoMudou'),
  folhaSemLink: f('skills.folhaSemLink'),
  folhaErro: (motivo: Valor) => f('skills.folhaErro', { motivo }),
  folhaRodape: f('skills.folhaRodape'),
  outraVersao: f('skills.outraVersao'),
  bloqueada: f('skills.bloqueada'),
  // Página Skills (cabeçalho, abas e rodapé).
  descricaoPagina: f('skills.descricaoPagina'),
  atualizarDica: f('skills.atualizarDica'),
  rodapePagina: f('skills.rodapePagina'),
  introCatalogo: f('skills.introCatalogo'),
  grupoCatalogo: f('skills.grupoCatalogo'),
  grupoSemRecibo: f('skills.grupoSemRecibo'),
  notaSemRecibo: f('skills.notaSemRecibo'),
  // Internas: vêm com o app e o Hub aplica sozinho; só leitura.
  grupoInternas: f('skills.grupoInternas'),
  notaInternas: f('skills.notaInternas'),
  usoInterna: { todaConversa: f('skills.usoInterna.todaConversa'), criarSkill: f('skills.usoInterna.criarSkill') } as Record<string, Frase>,
  usoInternaOutro: f('skills.usoInterna.outro'),
  nenhumaInstalada: f('skills.nenhumaInstalada'),
  nenhumaInstaladaDetalhe: f('skills.nenhumaInstaladaDetalhe'),
  escreverHubMudou: f('skills.escreverHubMudou'),
  escreverOutraSalvando: f('skills.escreverOutraSalvando'),
  detalhesConexaoMudou: f('skills.detalhesConexaoMudou'),
  // Detalhes de uma instalada: a versão de agora no catálogo, à parte.
  outraVersaoNoCatalogo: f('skills.outraVersaoNoCatalogo'),
  instrucoesInstalada: f('skills.instrucoesInstalada'),
  instrucoesCatalogo: f('skills.instrucoesCatalogo'),
  escreverRodape: f('skills.escreverRodape'),
  // d2a / DON-855: comparar rascunho, sem efeito. Nada aqui salva, aprova ou reserva revisão.
  revisao: {
    entrada: f(`${R}entrada`),
    entradaRodape: f(`${R}entradaRodape`),
    titulo: f(`${R}entrada`),
    semEfeito: f(`${R}semEfeito`),
    semEfeitoDetalhe: f(`${R}semEfeitoDetalhe`),
    lendo: f(`${R}lendo`),
    rascunho: f(`${R}rascunho`),
    rascunhoSelo: f(`${R}rascunhoSelo`),
    alterado: f(`${R}alterado`),
    nome: (id: string) => f(`${R}nome`, { id }),
    tamanho: (kib: string) => f(`${R}tamanho`, { kib }),
    grandeDemais: f(`${R}grandeDemais`),
    comparar: f(`${R}comparar`),
    comparando: f(`${R}comparando`),
    compararNota: f(`${R}compararNota`),
    comparacao: f(`${R}comparacao`),
    lida: f(`${R}lida`),
    abas: { diff: f(`${R}abas.diff`), base: (n: number) => f(`${R}abas.base`, { n }), rascunho: (n: number) => f(`${R}abas.rascunho`, { n }) },
    resumo: (total: number) => f(`${R}resumo`, { total }),
    diffRodape: f(`${R}diffRodape`),
    compareDeNovo: f(`${R}compareDeNovo`),
    compareDeNovoDetalhe: f(`${R}compareDeNovoDetalhe`),
    bots: f(`${R}bots`),
    nenhumBot: f(`${R}nenhumBot`),
    botsRodape: f(`${R}botsRodape`),
    tecnico: f(`${R}tecnico`),
    tecnicoInstalada: (n: number) => f(`${R}tecnicoInstalada`, { n }),
    tecnicoRascunho: (n: number) => f(`${R}tecnicoRascunho`, { n }),
    tecnicoRodape: f(`${R}tecnicoRodape`),
    rodape: f(`${R}rodape`),
    voltar: f(`${R}voltar`),
    lerDeNovo: f(`${R}lerDeNovo`),
    tentar: f(`${R}tentar`),
    conexaoMudou: f(`${R}conexaoMudou`),
    conexaoMudouDetalhe: f(`${R}conexaoMudouDetalhe`),
    semConexao: f(`${R}semConexao`),
    semConexaoDetalhe: f(`${R}semConexaoDetalhe`),
    outroHub: f(`${R}outroHub`),
    retidoOutroHub: f(`${R}retidoOutroHub`),
    retidoOutraInstalacao: f(`${R}retidoOutraInstalacao`),
    outroHubDetalhe: f(`${R}outroHubDetalhe`),
    falha: {
      skill_review_base_unknown: falha('skill_review_base_unknown'),
      skill_review_manual_only: falha('skill_review_manual_only'),
      skill_review_base_changed: falha('skill_review_base_changed'),
      skill_review_scope_unavailable: falha('skill_review_scope_unavailable'),
      skill_review_invalid: falha('skill_review_invalid'),
      skill_installation_stale: falha('skill_installation_stale'),
      skill_installation_changed: falha('skill_installation_changed'),
      skill_installation_unavailable: falha('skill_installation_unavailable'),
      hub_sem_revisao: falha('hub_sem_revisao'),
      resposta_invalida: falha('resposta_invalida'),
    },
    falhaGenerica: (acao: 'ler' | 'comparar', motivo: Valor) => f(acao === 'ler' ? `${R}falhaLer` : `${R}falhaComparar`, { motivo }),
  },
}

/** Desconectado ou recusado: o título e o detalhe certos para cada caso. */
export function semHub(kind: string, hub: string) {
  return kind === 'unauthorized'
    ? { titulo: frases.semAcesso, detalhe: frases.semAcessoDetalhe(hub) }
    : { titulo: frases.semConexao, detalhe: frases.semConexaoDetalhe(hub) }
}

export const rotuloAcao = {
  conferir: f('skills.acao.conferir'),
  instalar: f('skills.acao.instalar'),
  instalando: f('skills.acao.instalando'),
  conferindo: f('skills.acao.conferindo'),
  instalada: f('skills.acao.instalada'),
  tentar: f('skills.acao.tentar'),
}

/** O objeto de frases já no idioma: cada `Frase` vira texto, e cada função que devolve `Frase` passa a devolver texto. */
export type Avaliado<T> = T extends Frase ? string
  : T extends (...a: infer A) => Frase ? (...a: A) => string
  : T extends readonly [Frase, Frase] ? [string, string]
  : T extends object ? { [K in keyof T]: Avaliado<T[K]> }
  : T

export function avaliar<T>(arvore: T, idioma: AppLanguage): Avaliado<T> {
  if (ehFrase(arvore)) return textoDe(arvore, idioma) as Avaliado<T>
  if (typeof arvore === 'function') return ((...a: unknown[]) => textoDe((arvore as (...x: unknown[]) => Frase)(...a), idioma)) as Avaliado<T>
  if (Array.isArray(arvore)) return arvore.map((x) => avaliar(x, idioma)) as Avaliado<T>
  if (arvore && typeof arvore === 'object') return Object.fromEntries(Object.entries(arvore).map(([k, v]) => [k, avaliar(v, idioma)])) as Avaliado<T>
  return arvore as Avaliado<T>
}

/** Os textos da biblioteca no idioma pedido, no formato de sempre (`textos.semConexao`, `textos.incerto.instalar(nome)`). */
export const textosEm = (idioma: AppLanguage) => avaliar(frases, idioma)
