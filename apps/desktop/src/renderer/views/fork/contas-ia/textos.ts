// D164: textos das contas de IA, caixa de frase, sem SSH, CLI ou horas. DON-917 (fatia 9a): vêm do catálogo
// (fork-i18n, módulo `contas`) no idioma pedido. `textosEm(idioma)` é puro (as regras e os testes do Node o usam);
// as telas usam `useTextosContas()` (use-textos.ts), que segue o idioma do app.
import type { AppLanguage } from '../../../../shared/fork-idioma.ts'
import { tradutor, type ChaveTexto } from '../../../lib/fork-i18n/index.ts'
import { isolado, ltr } from '../../../../shared/bidi.ts'

// Nome vindo do usuário ou do Hub dentro de uma frase: `isolado` (FSI…PDI). Domínio e horário: `ltr` (LRI…PDI).
const iso = isolado

export function textosEm(idioma: AppLanguage) {
  const t = tradutor(idioma)
  return {

    pagina: t('contas.pagina'),

    // D169: motores do Hub (os agentes que ficavam em Extensões), na mesma página.
    motoresTitulo: t('contas.motoresTitulo'),
    motoresRodape: t('contas.motoresRodape'),
    motoresAtualizar: t('contas.motoresAtualizar'),
    motoresCarregando: t('contas.motoresCarregando'),
    motoresErro: t('contas.motoresErro'),
    motoresDesconectado: t('contas.motoresDesconectado'),
    motoresVazioTitulo: t('contas.motoresVazioTitulo'),
    motoresVazioDetalhe: t('contas.motoresVazioDetalhe'),
    motoresInstalados: t('contas.motoresInstalados'),
    motoresParaInstalar: t('contas.motoresParaInstalar'),
    hub: t('contas.hub'),
    ligadoPorEsteMac: t('contas.ligadoPorEsteMac'),
    rodape: t('contas.rodape'),
    vazioTitulo: t('contas.vazioTitulo'),
    vazioDetalhe: t('contas.vazioDetalhe'),
    adicionar: t('contas.adicionar'),
    atualizar: t('contas.atualizar'),
    carregando: t('contas.carregando'),
    hubDesconectado: t('contas.hubDesconectado'),
    donoPelaNuvem: t('contas.donoPelaNuvem'),
    remotoIndisponivel: t('contas.remotoIndisponivel'),
    perfilAntigo: t('contas.perfilAntigo'),
    perfilAntigoDetalhe: t('contas.perfilAntigoDetalhe'),
    usadoPor: (nomes: string) => t('contas.usadoPor', { nomes }),
    nenhumBot: t('contas.nenhumBot'),
    maisBots: (n: number) => t.plural('contas.maisBots', n),

    // Entrega 3: bots e modelos dentro da conta.
    botsTitulo: t('contas.botsTitulo'),
    botsVazioDetalhe: t('contas.botsVazioDetalhe'),
    botModeloPadrao: t('contas.botModeloPadrao'),
    modelosTitulo: t('contas.modelosTitulo'),
    modelosResumo: (visiveis: number, total: number) => t('contas.modelosResumo', { visiveis, total }),
    modelosDica: t('contas.modelosDica'),
    ocultar: t('contas.ocultar'),
    mostrar: t('contas.mostrar'),
    ocultarRotulo: (modelo: string) => t('contas.ocultarRotulo', { modelo: iso(modelo) }),
    mostrarRotulo: (modelo: string) => t('contas.mostrarRotulo', { modelo: iso(modelo) }),
    oculto: t('contas.oculto'),
    ocultarSemHub: t('contas.ocultarSemHub'),
    ocultosTitulo: t('contas.ocultosTitulo'),
    ocultarEmUsoTitulo: (modelo: string) => t('contas.ocultarEmUsoTitulo', { modelo: iso(modelo) }),
    ocultarEmUsoMensagem: (nomes: string) => t('contas.ocultarEmUsoMensagem', { nomes }),
    modeloUsadoPor: (nomes: string) => t('contas.modeloUsadoPor', { nomes }),
    modelosVazioTitulo: t('contas.modelosVazioTitulo'),
    modelosVazioDetalhe: t('contas.modelosVazioDetalhe'),
    modelosVerificar: t('contas.modelosVerificar'),

    // Estado e cobrança
    conectada: t('contas.conectada'),
    naoConfirmada: t('contas.naoConfirmada'),
    indisponivel: t('contas.indisponivel'),
    desconectada: t('contas.desconectada'),
    semLogin: t('contas.semLogin'),
    assinatura: t('contas.assinatura'),
    apiPaga: t('contas.apiPaga'),
    cobrancaNaoConfirmada: t('contas.cobrancaNaoConfirmada'),
    detalheNaoConfirmada: t('contas.detalheNaoConfirmada'),
    detalheIndisponivel: t('contas.detalheIndisponivel'),
    detalheDesconectada: t('contas.detalheDesconectada'),
    detalheConectada: t('contas.detalheConectada'),
    detalheSemLogin: t('contas.detalheSemLogin'),
    detalheSemChave: t('contas.detalheSemChave'),

    // Ações
    conectar: t('contas.conectar'),
    continuar: t('contas.continuar'),
    conectando: t('contas.conectando'),
    reconectar: t('contas.reconectar'),
    informarChave: t('contas.informarChave'),
    trocarChave: t('contas.trocarChave'),
    renomear: t('contas.renomear'),
    desconectar: t('contas.desconectar'),
    remover: t('contas.remover'),
    detalhes: t('contas.detalhes'),
    concluir: t('contas.concluir'),
    fechar: t('contas.fechar'),
    cancelar: t('contas.cancelar'),
    salvar: t('contas.salvar'),
    tentarDeNovo: t('contas.tentarDeNovo'),
    comecar: t('contas.comecar'),
    comecarDeNovo: t('contas.comecarDeNovo'),
    gerarNovoCodigo: t('contas.gerarNovoCodigo'),
    abrirPagina: t('contas.abrirPagina'),
    copiarCodigo: t('contas.copiarCodigo'),
    codigoCopiado: t('contas.codigoCopiado'),
    enviarCodigo: t('contas.enviarCodigo'),
    abrirTerminal: t('contas.abrirTerminal'),
    maisAcoes: t('contas.maisAcoes'),

    // Nova conta
    novaContaTitulo: t('contas.novaContaTitulo'),
    novaContaSub: (hub: string) => t('contas.novaContaSub', { hub: iso(hub) }),
    qualMotor: t('contas.qualMotor'),
    comoEntra: t('contas.comoEntra'),
    porLogin: t('contas.porLogin'),
    porLoginDetalhe: t('contas.porLoginDetalhe'),
    porChave: t('contas.porChave'),
    porChaveDetalhe: t('contas.porChaveDetalhe'),
    rotulo: t('contas.rotulo'),
    rotuloDica: t('contas.rotuloDica'),
    criarEConectar: t('contas.criarEConectar'),
    criando: t('contas.criando'),

    // Renomear, desconectar, remover
    renomearTitulo: t('contas.renomearTitulo'),
    desconectarTitulo: (nome: string) => t('contas.desconectarTitulo', { nome: iso(nome) }),
    desconectarMensagem: t('contas.desconectarMensagem'),
    removerTitulo: (nome: string) => t('contas.removerTitulo', { nome: iso(nome) }),
    removerMensagem: t('contas.removerMensagem'),
    removerComBots: (nomes: string) => t('contas.removerComBots', { nomes }),

    // Ações à vista na linha: testar (sem mensagem e sem custo) e excluir com confirmação.
    testar: t('contas.testar'),
    testando: t('contas.testando'),
    testeOk: t('contas.testeOk'),
    testeFalhou: (motivo: string) => t('contas.testeFalhou', { motivo }),
    excluir: t('contas.excluir'),
    excluirTitulo: t('contas.excluirTitulo'),
    excluirMensagemLivre: (nome: string) => t('contas.excluirMensagemLivre', { nome: iso(nome) }),
    excluirMensagemAntigo: (nome: string) => t('contas.excluirMensagemAntigo', { nome: iso(nome) }),
    excluirEmUsoTitulo: t('contas.excluirEmUsoTitulo'),
    excluirEmUsoMensagem: (nomes: string) => t('contas.excluirEmUsoMensagem', { nomes }),

    // Painel de conexão
    conectarTitulo: (nome: string) => t('contas.conectarTitulo', { nome: iso(nome) }),
    reconectarTitulo: (nome: string) => t('contas.reconectarTitulo', { nome: iso(nome) }),
    ondeRoda: (motor: string, hub: string) => t('contas.ondeRoda', { motor: iso(motor), hub: iso(hub) }),
    introLogin: (motor: string) => t('contas.introLogin', { motor: iso(motor) }),
    introReconectar: t('contas.introReconectar'),
    introSemCusto: t('contas.introSemCusto'),
    etapas: [t('contas.etapa.preparando'), t('contas.etapa.aguardando'), t('contas.etapa.verificando'), t('contas.etapa.conectado')] as const,
    fasePreparando: (motor: string) => t('contas.fasePreparando', { motor: iso(motor) }),
    faseInstalando: (motor: string) => t('contas.faseInstalando', { motor: iso(motor) }),
    faseAguardando: t('contas.faseAguardando'),
    faseVerificando: t('contas.faseVerificando'),
    faseConectado: t('contas.faseConectado'),
    faseCancelando: t('contas.faseCancelando'),
    faseCancelado: t('contas.faseCancelado'),
    faseExpirado: t('contas.faseExpirado'),
    faseFalhou: t('contas.faseFalhou'),
    semResposta: t('contas.semResposta'),
    escolhaNoBot: t('contas.escolhaNoBot'),

    // Desafio
    paginaOficial: (dominio: string) => t('contas.paginaOficial', { dominio: ltr(dominio) }),
    digiteCodigo: t('contas.digiteCodigo'),
    colarCodigo: t('contas.colarCodigo'),
    codigoDaPagina: t('contas.codigoDaPagina'),
    valeAte: (hora: string) => t('contas.valeAte', { hora: ltr(hora) }),
    venceu: t('contas.venceu'),
    enderecoRecusado: t('contas.enderecoRecusado'),
    enviando: t('contas.enviando'),

    // Terminal
    terminalTitulo: t('contas.terminalTitulo'),
    terminalAviso: t('contas.terminalAviso'),
    terminalVoltar: t('contas.terminalVoltar'),
    terminalCancelar: t('contas.terminalCancelar'),
    terminalAtivo: (hub: string) => t('contas.terminalAtivo', { hub: iso(hub) }),
    terminalEncerrado: t('contas.terminalEncerrado'),
    terminalVencido: t('contas.terminalVencido'),
    terminalIndisponivel: t('contas.terminalIndisponivel'),

    // Chave de API
    chaveTitulo: (nome: string) => t('contas.chaveTitulo', { nome: iso(nome) }),
    chaveSub: (empresa: string, hub: string) => t('contas.chaveSub', { empresa: iso(empresa), hub: iso(hub) }),
    chaveCarregando: t('contas.chaveCarregando'),
    chaveSemCampos: t('contas.chaveSemCampos'),
    chaveSalva: t('contas.chaveSalva'),
    chaveNotas: t('contas.chaveNotas'),
    chaveCusto: t('contas.chaveCusto'),
    chaveOpcional: (rotulo: string) => t('contas.chaveOpcional', { rotulo: iso(rotulo) }),
    pegarChave: t('contas.pegarChave'),
    salvando: t('contas.salvando'),

    // Uso
    uso: t('contas.uso'),
    usoIndisponivel: t('contas.usoIndisponivel'),
    usoPerto: t('contas.usoPerto'),
    // MEX-1391/F6-B: "Se esta conta acabar" (ordem de reserva do Space para o motor).
    contaDesconhecida: t('contas.contaDesconhecida'),
    reservaTitulo: t('contas.reserva.titulo'),
    reservaContagem: (n: number, max: number) => t('contas.reserva.contagem', { n, max }),
    reservaPassarPara: t('contas.reserva.passarPara'),
    reservaVazia: t('contas.reserva.vazia'),
    reservaAdicionar: t('contas.reserva.adicionar'),
    reservaSemOpcoes: t('contas.reserva.semOpcoes'),
    reservaLimite: (max: number) => t('contas.reserva.limite', { max }),
    reservaSubir: t('contas.reserva.subir'),
    reservaDescer: t('contas.reserva.descer'),
    reservaTirar: t('contas.reserva.tirar'),
    reservaNota: t('contas.reserva.nota'),
    reservaDescansaPerto: (quando: string) => t('contas.reserva.descansaPerto', { quando: ltr(quando) }),
    reservaDescansaEsgotada: (quando: string) => t('contas.reserva.descansaEsgotada', { quando: ltr(quando) }),
    reservaVoltarAgora: t('contas.reserva.voltarAgora'),
    reservaUsoExtra: t('contas.reserva.usoExtra'),
    reservaUsoExtraAte: (quando: string) => t('contas.reserva.usoExtraAte', { quando: ltr(quando) }),
    reservaBloqueada: t('contas.reserva.bloqueada'),
    reservaNa: (reserva: string, origem: string) => t('contas.reserva.na', { reserva, origem }),
    // MEX-1294/F6-A: rótulos das janelas normalizadas; id novo do Hub cai no texto cru.
    usoJanela: (id: string) =>
      id === 'session' ? t('contas.usoSessao') : id === 'week' ? t('contas.usoSemana') : id.replace(/[_-]+/g, ' '),

    // Ajuda no catálogo herdado
    avisoCatalogo: t('contas.avisoCatalogo'),
  }
}

export type TextosContas = ReturnType<typeof textosEm>

/** Códigos de erro do Hub (`código: mensagem`) com frase própria. Os outros usam a mensagem do Hub. */
const ERROS = [
  'remote_setup_unavailable', 'setup_authority_lost', 'operation_forbidden', 'not_allowed', 'account_conflict',
  'request_conflict', 'account_busy', 'account_in_use', 'account_invalid', 'account_not_found', 'account_incompatible',
  'accounts_unavailable', 'legacy_preserved', 'profile_invalid', 'setup_in_progress', 'api_form_required',
  'operation_expired', 'operation_stale', 'operation_limit', 'auth_code_invalid', 'auth_input_unavailable', 'invalid_field',
  'request_pending',
] as const
/** Motivos da fase final sem sucesso (`errorCode` da operação). */
const FALHAS = [
  'setup_cancelled', 'setup_expired', 'auth_url_invalid', 'auth_correlation_invalid', 'auth_capability_unavailable',
  'auth_protocol_invalid', 'auth_output_limit', 'auth_failed', 'cli_incompatible', 'account_busy', 'account_unverified',
  'identity_changed', 'account_conflict', 'install_integrity_invalid', 'grok_not_installed', 'setup_failed',
] as const

/** A chave do catálogo para um código de erro do Hub, ou `null` (a tela mostra a mensagem do Hub). */
export function chaveDeErro(code: string | null): ChaveTexto | null {
  if (code === 'terminal_unavailable') return 'contas.terminalIndisponivel'
  return code && (ERROS as readonly string[]).includes(code) ? `contas.erro.${code as (typeof ERROS)[number]}` : null
}

/** A chave do motivo de uma operação que terminou sem sucesso, ou `null`. */
export function chaveDeFalha(code: string | null): ChaveTexto | null {
  if (code && (FALHAS as readonly string[]).includes(code)) return `contas.falha.${code as (typeof FALHAS)[number]}`
  return chaveDeErro(code)
}
