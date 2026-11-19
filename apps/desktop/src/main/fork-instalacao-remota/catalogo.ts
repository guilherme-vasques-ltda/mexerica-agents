// MEX-1307 F0: do código (estado, motivo, evento) à chave do catálogo de textos (renderer/lib/fork-i18n, módulo
// `instalacaoRemota`). O main nunca monta texto: só chaves tipadas (o compilador confere que cada chave existe); texto de
// erro vem do catálogo do app, nunca da VPS (§4.6).
import type { ChaveTexto } from '../../renderer/lib/fork-i18n/index.ts'
import type { CodigoEvento, EstadoEvento } from './esquemas.ts'
import type { Estado, MotivoBloqueio } from './maquina.ts'
import type { MotivoIncompativel } from './sondagem.ts'

export const CHAVE_ESTADO: Readonly<Record<Estado, ChaveTexto>> = {
  Nova: 'instalacaoRemota.estado.nova', Alcancando: 'instalacaoRemota.estado.alcancando', ConfirmandoChave: 'instalacaoRemota.estado.confirmandoChave',
  Entrando: 'instalacaoRemota.estado.entrando', Verificando: 'instalacaoRemota.estado.verificando', SondandoRoot: 'instalacaoRemota.estado.sondandoRoot',
  Incompativel: 'instalacaoRemota.estado.incompativel', Bloqueada: 'instalacaoRemota.estado.bloqueada', Resumo: 'instalacaoRemota.estado.resumo',
  PedindoSenha: 'instalacaoRemota.estado.pedindoSenha', SenhaErrada: 'instalacaoRemota.estado.senhaErrada', Enviando: 'instalacaoRemota.estado.enviando',
  ConferindoRoot: 'instalacaoRemota.estado.conferindoRoot', Instalando: 'instalacaoRemota.estado.instalando', Saude: 'instalacaoRemota.estado.saude',
  Conectando: 'instalacaoRemota.estado.conectando', Pronta: 'instalacaoRemota.estado.pronta', Interrompida: 'instalacaoRemota.estado.interrompida',
  PacoteRuim: 'instalacaoRemota.estado.pacoteRuim', Falhou: 'instalacaoRemota.estado.falhou', Limpa: 'instalacaoRemota.estado.limpa',
  Parcial: 'instalacaoRemota.estado.parcial', IntervencaoManual: 'instalacaoRemota.estado.intervencaoManual', Desinstalando: 'instalacaoRemota.estado.desinstalando',
  Desinstalada: 'instalacaoRemota.estado.desinstalada', DesinstalacaoParcial: 'instalacaoRemota.estado.desinstalacaoParcial', Cancelada: 'instalacaoRemota.estado.cancelada',
}

export const CHAVE_INCOMPATIVEL: Readonly<Record<MotivoIncompativel, ChaveTexto>> = {
  so: 'instalacaoRemota.incompativel.so', arquitetura: 'instalacaoRemota.incompativel.arquitetura', systemd: 'instalacaoRemota.incompativel.systemd',
  espaco: 'instalacaoRemota.incompativel.espaco', sudo: 'instalacaoRemota.incompativel.sudo', sudotty: 'instalacaoRemota.incompativel.sudotty',
  libcap: 'instalacaoRemota.incompativel.libcap', nosuid: 'instalacaoRemota.incompativel.nosuid', hardlinks: 'instalacaoRemota.incompativel.hardlinks',
  relogio: 'instalacaoRemota.incompativel.relogio', 'sudo-indisponivel': 'instalacaoRemota.incompativel.sudoIndisponivel',
  'login-root': 'instalacaoRemota.incompativel.loginRoot', 'login-e-hub': 'instalacaoRemota.incompativel.loginEHub',
  'hub-uid-zero': 'instalacaoRemota.incompativel.hubUidZero', identidade: 'instalacaoRemota.incompativel.identidade',
  'porta-ocupada': 'instalacaoRemota.incompativel.portaOcupada', 'porta-desconhecida': 'instalacaoRemota.incompativel.portaDesconhecida', 'saida-invalida': 'instalacaoRemota.incompativel.saidaInvalida',
  'estado-ilegivel': 'instalacaoRemota.incompativel.estadoIlegivel',
}

export const CHAVE_BLOQUEIO: Readonly<Record<MotivoBloqueio, ChaveTexto>> = {
  'sem-comparacao': 'instalacaoRemota.bloqueio.semComparacao', 'chave-mudou': 'instalacaoRemota.bloqueio.chaveMudou',
}

/** Eventos exibíveis (§4.6): só estes códigos viram linha de progresso; os demais alimentam a decisão e nunca aparecem. */
export const CHAVE_EVENTO_EXIBIVEL: Readonly<Partial<Record<CodigoEvento, ChaveTexto>>> = {
  usuario: 'instalacaoRemota.evento.usuario', grupo: 'instalacaoRemota.evento.grupo', ajudante: 'instalacaoRemota.evento.ajudante',
  unidade: 'instalacaoRemota.evento.unidade', sudoers: 'instalacaoRemota.evento.sudoers', linger: 'instalacaoRemota.evento.linger',
  contahub: 'instalacaoRemota.evento.contahub', chaveapp: 'instalacaoRemota.evento.chaveapp', iniciar: 'instalacaoRemota.evento.iniciar',
  saude: 'instalacaoRemota.evento.saude', remocao: 'instalacaoRemota.evento.remocao', transferencia: 'instalacaoRemota.evento.transferencia',
}
export const CHAVE_ESTADO_EVENTO: Readonly<Record<EstadoEvento, ChaveTexto>> = {
  ok: 'instalacaoRemota.eventoEstado.ok', pulado: 'instalacaoRemota.eventoEstado.pulado', falha: 'instalacaoRemota.eventoEstado.falha',
}

/** Mensagens fixas da tela (a F4 desenha; a F0 só fixa as chaves). */
export const CHAVE_MENSAGEM = {
  senhaErrada: 'instalacaoRemota.senha.errada', senhaEsgotada: 'instalacaoRemota.senha.esgotada', faseBAguarde: 'instalacaoRemota.faseB.aguarde',
  retomada: 'instalacaoRemota.resumo.retomada', nadaMudou: 'instalacaoRemota.resumo.nadaMudou', mudou: 'instalacaoRemota.resumo.mudou',
  protegido: 'instalacaoRemota.resumo.protegido', hostExistente: 'instalacaoRemota.resumo.hostExistente', intervencao: 'instalacaoRemota.intervencao.explica',
} as const satisfies Record<string, ChaveTexto>
