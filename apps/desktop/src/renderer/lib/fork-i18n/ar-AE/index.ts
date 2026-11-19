import { textos as vozAjustes } from './voz/ajustes.ts'
import { textos as vozChamada } from './voz/chamada.ts'
import { textos as ajustes } from './ajustes.ts'
import { textos as conversaAjustes } from './conversa/ajustes.ts'
import { textos as conversaArquivar } from './conversa/arquivar.ts'
import type { Catalogo } from '../index.ts'
import { textos as aparencia } from './aparencia.ts'
import { textos as armazenamento } from './armazenamento.ts'
import { textos as botCreate } from './botCreate.ts'
import { textos as botRoles } from './botRoles.ts'
import { textos as botSettings } from './botSettings.ts'
import { textos as botsEditor } from './bots/editor.ts'
import { textos as botsPainel } from './bots/painel.ts'
import { textos as botsAjustes } from './bots/ajustes.ts'
import { textos as conta } from './conta.ts'
import { textos as contas } from './contas.ts'
import { textos as torre } from './torre.ts'
import { textos as fundo } from './fundo.ts'
import { textos as lugares } from './lugares.ts'
import { textos as instalacaoRemota } from './instalacaoRemota.ts'
import { textos as contextoCusto } from './contexto/custo.ts'
import { textos as contextoMedidor } from './contexto/medidor.ts'
import { textos as contrato } from './contrato.ts'
import { textos as projetos } from './projetos.ts'
import { textos as conversaCusto } from './conversa/custo.ts'
import { textos as conversaFila } from './conversa/fila.ts'
import { textos as conversaAprovacao } from './conversa/aprovacao.ts'
import { textos as conversaCompositor } from './conversa/compositor.ts'
import { textos as conversaMensagens } from './conversa/mensagens.ts'
import { textos as conversaAvisos } from './conversa/avisos.ts'
import { textos as conversaCartoes } from './conversa/cartoes.ts'
import { textos as conversaEscolha } from './conversa/escolha.ts'
import { textos as conversaRecibo } from './conversa/recibo.ts'
import { textos as conversaSkills } from './conversa/skills.ts'
import { textos as conversaLimites } from './conversa/limites.ts'
import { textos as conversaSeletor } from './conversa/seletor.ts'
import { textos as conversaTroca } from './conversa/troca.ts'
import { textos as conversaNavegacao } from './conversa/navegacao.ts'
import { textos as delegacao } from './delegacao.ts'
import { textos as nuncaFazer } from './nuncaFazer.ts'
import { textos as grupos } from './grupos.ts'
import { textos as conectoresCartao } from './conectores/cartao.ts'
import { textos as skillsCatalogo } from './skills/catalogo.ts'
import { textos as extensoesGeral } from './extensoes/geral.ts'
import { textos as entrada } from './entrada.ts'
import { textos as idioma } from './idioma.ts'
import { textos as marca } from './marca.ts'
import { textos as navegacaoJanela } from './navegacao/janela.ts'
import { textos as navegacaoBarra } from './navegacao/barra.ts'
import { textos as navegacaoIlha } from './navegacao/ilha.ts'
import { textos as navegacaoParear } from './navegacao/parear.ts'
import { textos as navegacaoConta } from './navegacao/conta.ts'
import { textos as memoriaAjustes } from './memoria/ajustes.ts'
import { textos as memoriaSugestoes } from './memoria/sugestoes.ts'
import { textos as memoriaPainel } from './memoria/painel.ts'
import { textos as memoriaAuto } from './memoria/auto.ts'
import { textos as rotinas } from './rotinas.ts'
import { textos as nuvem } from './nuvem.ts'
import { textos as sessions } from './sessions.ts'
import { textos as tema } from './tema.ts'
import { textos as times } from './times.ts'
import { textos as timesQuadro } from './times/quadro.ts'
import { textos as timesRegras } from './times/regras.ts'
import { textos as spaces } from './spaces.ts'
import { textos as mercado } from './mercado.ts'
import { textos as mercadoGoogle } from './mercadoGoogle.ts'
import { textos as mercadoPersonalizado } from './mercadoPersonalizado.ts'
import { textos as whatsappConversa } from './whatsapp/conversa.ts'
import { textos as whatsappNumero } from './whatsapp/numero.ts'
import { textos as whatsappPrazo } from './whatsapp/prazo.ts'
import { textos as whatsappVoz } from './whatsapp/voz.ts'
import { textos as whatsappEstados } from './whatsapp/estados.ts'
import { textos as whatsappPareamento } from './whatsapp/pareamento.ts'
import { textos as whatsappAjustes } from './whatsapp/ajustes.ts'
import { textos as whatsappCaixa } from './whatsapp/caixa.ts'
import { textos as whatsappFila } from './whatsapp/fila.ts'
import { textos as sistemaBarra } from './sistema/barra.ts'
import { textos as sistemaConversa } from './sistema/conversa.ts'
import { textos as webLogin } from './web/login.ts'

// Composição estática, um módulo por área (registro.ts). O gate confere cada parte antes do agregado:
// chave duplicada entre partes, chave fora do prefixo do módulo, módulo esquecido ou não registrado.
export const arAE: Catalogo = {
  ...vozAjustes,
  ...vozChamada,
  ...ajustes,
  ...aparencia,
  ...armazenamento,
  ...botCreate,
  ...botRoles,
  ...botSettings,
  ...botsEditor,
  ...botsPainel,
  ...botsAjustes,
  ...conta,
  ...contas,
  ...torre,
  ...fundo,
  ...lugares,
  ...instalacaoRemota,
  ...contextoCusto,
  ...contextoMedidor,
  ...contrato,
  ...projetos,
  ...conversaAjustes,
  ...conversaArquivar,
  ...conversaCusto,
  ...conversaFila,
  ...conversaAprovacao,
  ...conversaCompositor,
  ...conversaMensagens,
  ...conversaAvisos,
  ...conversaCartoes,
  ...conversaEscolha,
  ...conversaRecibo,
  ...conversaSkills,
  ...conversaLimites,
  ...conversaSeletor,
  ...conversaTroca,
  ...conversaNavegacao,
  ...delegacao,
  ...nuncaFazer,
  ...grupos,
  ...conectoresCartao,
  ...skillsCatalogo,
  ...extensoesGeral,
  ...entrada,
  ...idioma,
  ...marca,
  ...navegacaoJanela,
  ...navegacaoBarra,
  ...navegacaoIlha,
  ...navegacaoParear,
  ...navegacaoConta,
  ...memoriaAjustes,
  ...memoriaSugestoes,
  ...memoriaPainel,
  ...memoriaAuto,
  ...rotinas,
  ...nuvem,
  ...sessions,
  ...tema,
  ...times,
  ...timesQuadro,
  ...timesRegras,
  ...spaces,
  ...mercado,
  ...mercadoGoogle,
  ...mercadoPersonalizado,
  ...whatsappConversa,
  ...whatsappNumero,
  ...whatsappPrazo,
  ...whatsappVoz,
  ...whatsappEstados,
  ...whatsappPareamento,
  ...whatsappAjustes,
  ...whatsappCaixa,
  ...whatsappFila,
  ...sistemaBarra,
  ...sistemaConversa,
  ...webLogin,
}
