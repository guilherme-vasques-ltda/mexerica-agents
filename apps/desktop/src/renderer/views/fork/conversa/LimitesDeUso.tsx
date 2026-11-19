// DON-907 C4: a seção "Limites de uso" de Ajustes → Conversa e os botões do aviso de espera no chat.
// Contrato do Hub e estados: docs/reference/design/don-907-c4/README.md. Interruptores e botões mostram só o que o Hub confirmou;
// nada fica decidido no app (sem temporizador, sem estado otimista). Textos por `useT` (`conversa.limites.*`): o estado
// guarda só códigos e a tradução acontece na renderização, então trocar o idioma não relê o Hub.
import { useCallback, useEffect, useRef, useState } from 'react'
import { Button, CardSection, Spinner } from '../../../components/Controls'
import { Icon } from '../../../components/Icon'
import { HStack } from '../../../components/Stack'
import { Text } from '../../../components/Text'
import { superficieIdioma, useIdioma, useT } from '../../../lib/fork-i18n/estado'
import { useModel } from '../../../lib/observable'
import type { BotStore } from '../../../store/bot-store'
import { useApp, useStore } from '../../../store/context'
import type { Entry } from '@shared/models'
import { SwitchRow } from '../../bots/parts'
import { acoesDaEspera, alvoDaEspera, hubAntigo, lerLimites, paradaDaEspera, type Acao, type LimitesDeUso } from './limites'
import '../idioma/escritas.css'
import './limites.css'

type Estado = { kind: 'lendo' } | { kind: 'pronto'; valor: LimitesDeUso } | { kind: 'falha'; falha: 'hubAntigo' | 'falhaLer' }

/** A seção para o `ConversaSettings` (C1): o Hub selecionado, senão o deste computador, como Armazenamento. */
export function LimitesDeUsoSection() {
  const app = useApp()
  const idioma = useIdioma()
  const t = useT()
  const store = app.selectedStore ?? app.local
  // Superfície já traduzida dentro da página ainda em português (docs/guides/idiomas.md): idioma e direção próprios.
  return (
    <div className="idioma-superficie" {...superficieIdioma(idioma)}>
      {store
        ? <Limites key={store.computer.id} source={store} />
        : <CardSection title={t('conversa.limites.titulo')}><Text tone="secondary">{t('conversa.limites.semConexao')}</Text></CardSection>}
    </div>
  )
}

function Limites({ source }: { source: BotStore }) {
  const t = useT()
  const store = useModel(source)
  const client = store.connection.kind === 'online' ? store.connectedClient : null
  const origem = `${store.hello?.hostId ?? ''}|${store.principalEpoch}`
  const [estado, setEstado] = useState<Estado>({ kind: 'lendo' })
  const [gravando, setGravando] = useState(false)
  const [falhaGravar, setFalhaGravar] = useState(false)
  const vez = useRef(0)

  const ler = useCallback(() => {
    if (!client) return
    const minha = ++vez.current
    client.call('donnaUsageLimits', {}).then(lerLimites).then(
      (valor) => { if (vez.current === minha) setEstado({ kind: 'pronto', valor }) },
      (e: unknown) => { if (vez.current === minha) setEstado({ kind: 'falha', falha: hubAntigo(e) ? 'hubAntigo' : 'falhaLer' }) },
    )
  }, [client]) // sem `t`: `useT` devolve outra função a cada render e a leitura do Hub entraria em laço

  // Outro Hub ou outro principal: começa do zero e lê de novo.
  useEffect(() => { vez.current++; setEstado({ kind: 'lendo' }); setGravando(false); setFalhaGravar(false) }, [origem])
  useEffect(() => { ler(); return () => { vez.current++ } }, [ler, origem])

  const gravar = (mudanca: Partial<LimitesDeUso>) => {
    if (!client || estado.kind !== 'pronto' || gravando) return
    const minha = ++vez.current
    setGravando(true)
    setFalhaGravar(false)
    client.call('donnaUsageLimitsSave', { ...estado.valor, ...mudanca }).then(lerLimites).then(
      (valor) => { if (vez.current === minha) { setEstado({ kind: 'pronto', valor }); setGravando(false) } },
      // Sem confirmação o Hub pode ter gravado ou não: libera antes de reler, e a releitura (que muda a vez) mostra
      // o valor que ele tem de fato.
      () => { if (vez.current === minha) { setGravando(false); setFalhaGravar(true); ler() } },
    )
  }

  const corpo = !client ? <Text tone="secondary">{t('conversa.limites.semConexao')}</Text>
    : estado.kind === 'lendo' ? <HStack gap={8}><Spinner /><Text tone="secondary">{t('conversa.limites.carregando')}</Text></HStack>
    : estado.kind === 'falha' ? (
      <HStack gap={10}>
        <Text tone="secondary" grow>{t(`conversa.limites.${estado.falha}`)}</Text>
        {estado.falha === 'hubAntigo' ? null : <Button kind="secondary" onClick={ler}>{t('conversa.limites.tentarDeNovo')}</Button>}
      </HStack>
    ) : null
  return (
    <CardSection title={t('conversa.limites.titulo')} footer={t(falhaGravar ? 'conversa.limites.falhaGravar' : 'conversa.limites.rodape')}>
      {corpo ?? (estado.kind === 'pronto' && [
        <SwitchRow key="retomar" title={t('conversa.limites.retomar')} detail={t('conversa.limites.retomarDetalhe')} on={estado.valor.autoResume} disabled={gravando} onChange={(autoResume) => gravar({ autoResume })} />,
        <SwitchRow key="soneca" title={t('conversa.limites.soneca')} detail={t('conversa.limites.sonecaDetalhe')} on={estado.valor.snooze} disabled={gravando} onChange={(snooze) => gravar({ snooze })} />,
      ])}
    </CardSection>
  )
}

/**
 * O aviso de espera do limite com os botões que o Hub anuncia (`subscriptionWait.waitId`). Na espera automática o texto
 * é "Parou pelo limite do plano; volta às HH:MM" (MX-245, a hora de `resumeAt` no idioma do app); nos outros estados
 * (cancelada, retomada desligada, prazo que chegou, pausada) é o do Hub. O app só acrescenta a nota de uma falha. Cancelar nunca usa `stop`.
 */
export function AvisoDeEspera({ entry }: { entry: Entry }) {
  const idioma = useIdioma()
  const t = useT()
  const store = useStore()
  const w = entry.data.subscriptionWait
  const acoes = acoesDaEspera(entry)
  const parada = paradaDaEspera(t, idioma, entry)
  const [ocupado, setOcupado] = useState(false)
  const [nota, setNota] = useState<Acao | null>(null)
  // Outra espera no mesmo aviso: a nota da anterior não vale para ela.
  useEffect(() => { setNota(null) }, [w?.waitId, w?.cancelled])

  const agir = async (acao: Acao) => {
    const alvo = alvoDaEspera(entry)
    if (!alvo || ocupado) return
    setOcupado(true)
    setNota(null)
    try {
      const client = await store.ready()
      if (acao === 'naoRetomar') {
        // Com `alreadyStarted`, o próprio aviso do Hub passa a dizer que a retomada já tinha sido autorizada.
        await client.call('donnaUsageWaitCancel', alvo)
      } else {
        await client.call('donnaUsageWaitResume', alvo)
      }
    } catch {
      setNota(acao)
    } finally {
      setOcupado(false)
    }
  }

  return (
    // O texto do Hub chega pronto, em português: fica marcado como tal; a frase da parada, os botões e a nota seguem o idioma.
    <div className="chat-notice idioma-superficie" {...superficieIdioma(idioma)}>
      <HStack gap={8} className={`espera-limite glass-raised${entry.data.style === 'error' ? ' espera-limite-pausada' : ''}`} role="status">
        <Icon name={w?.snoozed && !w.cancelled ? 'moon' : 'clock'} size={12} color="var(--context-ink-2, var(--ink-2))" />
        {parada
          ? <Text size="footnote" tone="secondary" className="selectable">{parada}</Text>
          : <Text size="footnote" tone="secondary" className="selectable bidi-isolado" lang="pt-BR" dir="auto">{entry.data.text ?? ''}</Text>}
        {acoes.map((acao) => (
          <Button key={acao} kind="secondary" disabled={ocupado} onClick={() => void agir(acao)}>{t(acao === 'naoRetomar' ? 'conversa.limites.naoRetomar' : 'conversa.limites.retomarAgora')}</Button>
        ))}
      </HStack>
      {nota ? <Text as="div" size="caption" tone="secondary" className="espera-limite-nota">{t(nota === 'naoRetomar' ? 'conversa.limites.falhaCancelar' : 'conversa.limites.falhaRetomar')}</Text> : null}
    </div>
  )
}
