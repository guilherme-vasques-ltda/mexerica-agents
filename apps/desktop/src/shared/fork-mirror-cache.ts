import { normalizeBot, unavailableRuntime, type Bot } from './models.ts'

// O fork pode mudar campos normalizados sem alterar a versão comercial do app/Hub.
// Incremente ao adicionar campos que um cliente anterior descartava (ex.: avatarConfig).
// Schema 2 reservado ao contrato de pastas (14.0), desenvolvido em paralelo.
// Schema 5: origem das ferramentas do Hub (#58). Schema 6: função do bot (D133).
// Schema 10: sessões por bot (Entry.sessionId e marcador de sessão).
// Schema 11: referência pública do ledger de conectores (#202; sem UI de decisão).
// Schema 12: procedência por versão e disposição da memória automática (#238; D246 A).
// Schema 13 reservado à privacidade (#232): status `unavailable` e Screen por Space.
// Runtime de cache anterior a 13 não é confiável; preservar os contratos já mesclados.
// Schema 14: condição opaca da seleção de skills (`Bot.skillSelection`, c1/D288).
// Schema 15: líder do canal (`Bot.leader`).
// Schema 16: perguntas do bot ao dono (`Bot.pendingQuestions`, MB-1b/D326, #356); cache anterior entra com [].
// Schema 17: cartão de conector com conta de saída e rótulos (`decisionProtocol` 2, Proposta B, #358).
// Schema 18: cartão de escolha do bot (`Entry.data.genuiChoice`, G1b/D273); cache anterior não tem a projeção.
// Schema 19: linha de troca de modelo (`Entry.data.modelSwitch`, DON-907 C9a); cache anterior não tem o campo.
// Schema 20: versão das regras de delegação no aviso do remetente (`Entry.data.delegationRulesVersion`, D371, #439);
// cache anterior não tem o campo.
// Schema 21: espera do limite com `waitId`/`cancelled`/`snoozed`/`autoResume`/`ready` (`Entry.data.subscriptionWait`, DON-907 C4).
// Schema 22: contrato da tarefa no pedido entre bots (`Entry.data.botMessage.contract`, T2a/DON-912, #467); cache
// anterior não tem o campo. Regra: quem entra usa main+1, conferido no último merge com a main.
// Schema 23: preferências do dono para o Berga (`Bot.ownerPreferences`, D368, #444).
// Schema 24: ordem da fila depois de reordenar (`Entry.data.queueOrder`, DON-907/D431).
// Schema 25: rascunho nativo WhatsApp não decisível (`Entry.data.conectorDraft`).
// Schema 26: natureza `confirmed` e ação `reclassify` no cartão de memória (`Entry.data.memory`, 2b1d-2).
// Schema 27: chave de tela por bot/computador (`Bot.screenAccess`, D510 parte 1).
// Schema 28: aparência versionada própria (Bot.mexeriquinha), mantendo avatarConfig legado.
// Schema 29: cartão da primeira vez da tela (D511 parte 2)
// Schema 30: aviso persistente de criação de rotina (`Notice.data.routineCreation`, D544).
// Schema 31: preferência de navegador do dono (Bot.browserPreference), sem permissão implícita.
// Schema 32: principal próprio no novo fluxo de Space (D543, fatia 1B), após o schema 31 do #629 na ordem coordenada.
// Schema 33: valor `off` da preferência de navegador do dono (Bot.browserPreference); app antigo lê como integrated.
// Schema 34: conjunto 3D gerado pelo Hub (Bot.mexeriquinha v3: assetId + fallbackConjuntoId).
// Schema 35: ciclo de vida do Space (D543 E1) — principal fixo "Berga" por Space, ativação e exclusão.
// Schema 36: lojas e identidade de seleção separadas por Space (E2).
// Schema 37: código estável da atividade do bot (`Bot.activityCode`, MX-147); cache anterior não tem o campo.
// Schema 38: origem e edição do pedido de bot na fila (`Entry.data.sourceChat`/`sourceSession`,
// `botMessage.edited`, MX-77); cache anterior não tem os campos da linha "De <bot>".
// Schema 39: MX-142 (Tarefas), após o 38 (MX-77). Tarefas NÃO entram no mirror: prioridade, prazo, fases, comentários,
// agenda e sessões vinculadas são lidos do Hub a cada abertura (projetos-session.ts, nada em disco). O bump só segue a
// regra de campo novo do Hub (docs/regras/pastas-e-codigo.md): não procure esses campos no cache.
// Schema 40: MX-211 — cartões de pedido de código de login ganham `origin` (vínculo da sessão) e o código não volta ao
// mirror nem ao histórico; cache anterior é descartado pelo próprio stamp.
// Schema 41: MX-105 — conversas de grupo trazem `groupAuto`, separado da marcação.
// Schema 42: motivo estruturado da falha de preparo (`Entry.data.reason` = {code, accountId?, backend?}, MX-315);
// cache anterior não tem o campo e cairia no texto genérico.
// Schema 43: MX-313 — estimatedCosts é lido do Hub sob demanda e não entra no mirror.
// Schema 45: mensagem de indisponibilidade de Organizar memória (MEX-1355).
// Schema 46: organização de grupos de bots (#939).
// Schema 47: MEX-1289 — confirmação efêmera do terminal de login, nunca gravada no mirror.
// Schema 48: o Hub deixou de mandar `hello.composioApps` e os conectores `kind: composio` (integração dedicada removida, #946).
// Schema 49: Marketplace 1a — o bot ganha `contaPadrao` (serviço → conta padrão) e `perguntarConta` (serviço → perguntar
// qual conta); `connectors` segue sendo a lista de contas ligadas. Cache anterior não tem os dois campos.
// Schema 50: o Hub manda `marca` (a chave da logo) em cada serviço do catálogo e em cada conta de conector; cache anterior
// não tem o campo e as logos voltariam ao globo até a próxima leitura.
// Schema 51: Google direto — o `marketCatalogo` traz `appsProprios.google` (só o ID do app da pessoa, nunca a chave) e as
// fichas do Google trazem `auth.modo: "app_proprio"` com o guia; cache anterior não tem o campo e o grupo "Google" não abriria.
// Schema 52: MEX-1391 (F6-B) — o aviso de troca de conta traz `Entry.data.contaTroca` ({kind, de, para, motivo, voltaEm}) e a
// falha de preparo ganha os códigos `quota_near_limit`, `quota_exhausted` e `fallback_no_proof` em `reason`; cache anterior
// não tem o campo nem sabe a frase desses códigos (cairia no texto genérico).
// Schema 53: Marketplace — o `marketCatalogo` passa a mandar `skills[]` das fichas como `{id, titulo, resumo}` (antes vinha
// vazia). O catálogo é lido sob demanda e não entra no mirror; o bump só segue a regra de campo novo do Hub. App antigo
// ignora a lista; o novo não mostra a seção quando ela não vem.
// Schema 54: GUI-192 — o aviso de motor mudo traz `Entry.data.motorMudo` ({estado, segundos}); cache anterior não tem o campo.
export const MIRROR_SCHEMA = 54

export const mirrorCacheStamp = (appVersion: string, hostVersion: string): string =>
  `donnabot-mirror-v${MIRROR_SCHEMA}/${appVersion}/${hostVersion}`

/** O cache foi gravado por um app com este schema (versões de app/Hub à parte). */
export const isCurrentMirrorSchema = (stamp: string | null | undefined): boolean =>
  typeof stamp === 'string' && stamp.startsWith(`donnabot-mirror-v${MIRROR_SCHEMA}/`)

/**
 * runtime-audiencia-01: bots lidos do cache. De um schema anterior, o runtime pode ter vindo de outro Space antes da
 * redação do Hub: entra como Estado indisponível até o Hub responder. Perfil, prévia e não lidas ficam; rascunhos e
 * histórico não passam por aqui.
 */
export function cachedBots(stamp: string | null | undefined, bots: Bot[]): Bot[] {
  const current = isCurrentMirrorSchema(stamp)
  return bots.map((bot) => normalizeBot(current ? bot : unavailableRuntime(bot)))
}
