// MEX-1391/F6-B: "Se esta conta acabar" nos Detalhes da conta — a ordem de reserva do Space para o motor desta conta
// ("Passar para"), o descanso ("volta sex 14:00") e os dois botões: Voltar agora e Permitir uso extra. O app só envia
// intenções; o Hub confere de novo (assinatura comprovada, mesmo motor, até 3) e é quem troca.
import { useEffect, useState } from 'react'
import type { ManagedAccount } from '../../../../shared/fork-engine-accounts.ts'
import { isolado } from '../../../../shared/bidi.ts'
import { Button, IconButton } from '../../../components/Controls'
import { Icon } from '../../../components/Icon'
import { HStack, Spacer, VStack } from '../../../components/Stack'
import { Text } from '../../../components/Text'
import { useModel } from '../../../lib/observable'
import { engineSetup } from '../../../store/fork-engine-setup'
import type { BotStore } from '../../../store/bot-store'
import type { ContasStore } from './contas-store'
import { reservaFor } from './reserva-store'
import { acrescentar, candidatas, emReservaPor, mover, MAX_RESERVAS, ordemDoMotor, situacaoDaConta, tirar } from './reserva'
import { errorMessage, whenText } from './regras'
import { useTextosContas } from './use-textos'
import { useIdioma } from '../../../lib/fork-i18n/estado'
import './reserva.css'

export function SeEstaContaAcabar({ store, contas, account }: { store: BotStore; contas: ContasStore; account: ManagedAccount }) {
  const textos = useTextosContas()
  const idioma = useIdioma()
  const reserva = useModel(reservaFor(store, () => store.ready(), engineSetup))
  const [adicionando, setAdicionando] = useState(false)
  const spaceId = store.spaceId
  useEffect(() => { void reserva.load(spaceId) }, [reserva, spaceId, account.id])

  // Só assinatura entra na troca automática; chave de API nunca (regra do Hub, repetida aqui para não oferecer).
  // Esta versão troca só a conversa do dono no Pessoal: em outro Space a seção nem aparece.
  const personal = store.spaces?.personalSpaceId
  if (account.billing !== 'subscription' || (spaceId && spaceId !== personal)) return null
  // Só o padrão de um bot do Space conta aqui (mais estreito que `botIds`, que inclui quem só roda na conta pelo fallback);
  // o Hub confere o vínculo de verdade ao salvar.
  const doSpace = (c: ManagedAccount) => c.botIds.some((id) => {
    const bot = store.bots.get(id)
    return bot?.forkEditor?.engineAccountId === c.id && (bot.spaceId ?? personal) === (spaceId ?? personal)
  })
  const label = (id: string) => contas.account(id)?.label ?? textos.contaDesconhecida
  const ordem = ordemDoMotor(reserva.state, account.backend)
  const opcoes = candidatas(contas.accounts ?? [], account.backend, ordem, account.id, doSpace)
  const situacao = situacaoDaConta(reserva.state, account.id)
  const emReserva = emReservaPor(reserva.state, account.id)
  const salvar = (nova: string[]) => void reserva.save(account.backend, nova, spaceId).catch(() => {})
  const agir = (action: 'backNow' | 'allowExtra') => void reserva.act(action, account.id, spaceId).catch(() => {})
  const volta = situacao?.restUntil ? whenText(situacao.restUntil, idioma) : null
  const podeUsoExtra = !!situacao?.restUntil && situacao.reason === 'perto_do_limite' && !situacao.extraUntil

  return (
    <section className="contas-secao reserva" aria-label={textos.reservaTitulo}>
      <HStack gap={6} className="contas-secao-titulo">
        <Text size="caption" weight="semibold" tone="secondary">{textos.reservaTitulo}</Text>
        <Spacer />
        <Text size="caption" tone="secondary" tabular>{textos.reservaContagem(ordem.length, MAX_RESERVAS)}</Text>
      </HStack>

      {situacao || emReserva.length ? (
        <div className="reserva-estado" role="status">
          <Icon name="clock" size={13} color="var(--text-attention, var(--attention))" />
          <VStack as="span" gap={2} grow shrink>
            {situacao?.restUntil ? (
              <Text tone="text">{situacao.reason === 'esgotada' ? textos.reservaDescansaEsgotada(volta ?? '') : textos.reservaDescansaPerto(volta ?? '')}</Text>
            ) : null}
            {situacao?.extraUntil ? <Text size="caption" tone="secondary">{textos.reservaUsoExtraAte(whenText(situacao.extraUntil, idioma))}</Text> : null}
            {situacao?.blockedUntil ? <Text size="caption" tone="secondary">{textos.reservaBloqueada}</Text> : null}
            {emReserva.length ? <Text size="caption" tone="secondary">{textos.reservaNa(isolado(label(emReserva[0]!.currentAccountId)), isolado(account.label))}</Text> : null}
          </VStack>
          <HStack gap={8}>
            {podeUsoExtra ? <Button kind="secondary" disabled={reserva.busy} onClick={() => agir('allowExtra')}>{textos.reservaUsoExtra}</Button> : null}
            {situacao?.restUntil ? <Button kind="secondary" disabled={reserva.busy} onClick={() => agir('backNow')}>{textos.reservaVoltarAgora}</Button> : null}
          </HStack>
        </div>
      ) : null}

      <Text size="caption" tone="secondary">{textos.reservaPassarPara}</Text>
      {ordem.length ? (
        <ol className="reserva-lista">
          {ordem.map((id, i) => (
            <li key={id} className="reserva-linha">
              <Text size="caption" tone="tertiary" tabular>{i + 1}</Text>
              <Text tone="text" weight="medium" grow truncate dir="auto">{label(id)}</Text>
              <IconButton title={textos.reservaSubir} icon="chevron.up" disabled={reserva.busy || i === 0} onClick={() => salvar(mover(ordem, id, -1))} />
              <IconButton title={textos.reservaDescer} icon="chevron.down" disabled={reserva.busy || i === ordem.length - 1} onClick={() => salvar(mover(ordem, id, 1))} />
              <IconButton title={textos.reservaTirar} icon="xmark" disabled={reserva.busy} onClick={() => salvar(tirar(ordem, id))} />
            </li>
          ))}
        </ol>
      ) : (
        <Text size="caption" tone="secondary" className="reserva-vazio">{textos.reservaVazia}</Text>
      )}

      {adicionando ? (
        <div className="reserva-opcoes">
          {opcoes.length ? opcoes.map((c) => (
            <button key={c.id} className="capsule-action" disabled={reserva.busy || ordem.length >= MAX_RESERVAS}
              onClick={() => { salvar(acrescentar(ordem, c.id)); setAdicionando(false) }}>
              <Icon name="plus" size={11} />{c.label}
            </button>
          )) : <Text size="caption" tone="secondary">{textos.reservaSemOpcoes}</Text>}
        </div>
      ) : (
        <HStack gap={8}>
          <Button kind="secondary" disabled={reserva.busy || ordem.length >= MAX_RESERVAS || !opcoes.length} onClick={() => setAdicionando(true)}>{textos.reservaAdicionar}</Button>
          {ordem.length >= MAX_RESERVAS ? <Text size="caption" tone="secondary">{textos.reservaLimite(MAX_RESERVAS)}</Text> : !opcoes.length ? <Text size="caption" tone="secondary">{textos.reservaSemOpcoes}</Text> : null}
        </HStack>
      )}
      <Text size="caption" tone="secondary">{textos.reservaNota}</Text>
      {reserva.error ? <Text tone="danger" role="alert" className="selectable">{errorMessage(reserva.error, idioma)}</Text> : null}
    </section>
  )
}
