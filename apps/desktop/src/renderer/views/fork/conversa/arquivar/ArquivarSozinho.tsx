// DON-907 C5: Ajustes → Conversa → "Arquivar sozinho". O prazo vive no Hub (`sessionAutoCloseConfig`/`Save`, com
// `expectedRevision`); o app só mostra o valor confirmado, sem timer nem preferência própria. Só o dono grava (o Hub
// confere); Hub antigo sem o RPC: a seção pede para atualizar. Textos por `useT` (`conversa.arquivar.*`).
import { useCallback, useEffect, useRef, useState } from 'react'
import { Button, CardSection, ChoicePicker, Spinner, ValueRow } from '../../../../components/Controls'
import { HStack } from '../../../../components/Stack'
import { Text } from '../../../../components/Text'
import { superficieIdioma, useIdioma, useT } from '../../../../lib/fork-i18n/estado'
import { useModel } from '../../../../lib/observable'
import type { BotStore } from '../../../../store/bot-store'
import { useApp } from '../../../../store/context'
import { DIAS_PADRAO, falhaDeAjuste, hubAntigo, lerConfig, NUNCA, opcoesDias, pedidoSalvar, type ArquivarConfig, type FalhaAjuste } from './controle'

type Estado = { kind: 'lendo' } | { kind: 'pronto'; valor: ArquivarConfig } | { kind: 'falha'; falha: 'hubAntigo' | 'falhaLer' }

export function ArquivarSozinhoSection() {
  const app = useApp()
  const idioma = useIdioma()
  const t = useT()
  const store = app.selectedStore ?? app.local
  return (
    <div className="idioma-superficie" {...superficieIdioma(idioma)}>
      {store
        ? <Arquivar key={store.computer.id} source={store} />
        : <CardSection title={t('conversa.arquivar.titulo')}><Text tone="secondary">{t('conversa.arquivar.semConexao')}</Text></CardSection>}
    </div>
  )
}

function Arquivar({ source }: { source: BotStore }) {
  const t = useT()
  const idioma = useIdioma()
  const store = useModel(source)
  const client = store.connection.kind === 'online' ? store.connectedClient : null
  const origem = `${store.hello?.hostId ?? ''}|${store.principalEpoch}`
  const [estado, setEstado] = useState<Estado>({ kind: 'lendo' })
  const [gravando, setGravando] = useState(false)
  const [falha, setFalha] = useState<FalhaAjuste | null>(null)
  const vez = useRef(0)

  const ler = useCallback(() => {
    if (!client) return
    const minha = ++vez.current
    client.call('sessionAutoCloseConfig', {}).then(lerConfig).then(
      (valor) => { if (vez.current === minha) setEstado({ kind: 'pronto', valor }) },
      (e: unknown) => { if (vez.current === minha) setEstado({ kind: 'falha', falha: hubAntigo(e) ? 'hubAntigo' : 'falhaLer' }) },
    )
  }, [client])

  useEffect(() => { vez.current++; setEstado({ kind: 'lendo' }); setGravando(false); setFalha(null) }, [origem])
  useEffect(() => { ler(); return () => { vez.current++ } }, [ler, origem])

  const gravar = (escolha: number) => {
    if (!client || estado.kind !== 'pronto' || gravando) return
    const minha = ++vez.current
    setGravando(true)
    setFalha(null)
    client.call('sessionAutoCloseSave', pedidoSalvar(escolha, estado.valor)).then(lerConfig).then(
      (valor) => { if (vez.current === minha) { setEstado({ kind: 'pronto', valor }); setGravando(false) } },
      // Conflito, recusa ou resposta incerta: relê e mostra o que o Hub tem de fato.
      (e: unknown) => { if (vez.current === minha) { setGravando(false); setFalha(falhaDeAjuste(e)); ler() } },
    )
  }

  const rotulo = (dias: number) => dias === NUNCA ? t('conversa.arquivar.nunca')
    : dias === DIAS_PADRAO ? t.plural('conversa.arquivar.diasPadrao', dias) : t.plural('conversa.arquivar.dias', dias)
  // O texto fixo fica sempre (decisão do dono); a falha da última gravação vem logo abaixo.
  const nota = falha === 'conflict' ? t('conversa.arquivar.conflito') : falha === 'notAllowed' ? t('conversa.arquivar.soDono')
    : falha === 'failed' ? t('conversa.arquivar.falhaGravar') : null
  const rodape = <>{t('conversa.arquivar.rodape')}{nota ? <><br /><span role="status">{nota}</span></> : null}</>

  const corpo = !client ? <Text tone="secondary">{t('conversa.arquivar.semConexao')}</Text>
    : estado.kind === 'lendo' ? <HStack gap={8}><Spinner /><Text tone="secondary">{t('conversa.arquivar.carregando')}</Text></HStack>
    : estado.kind === 'falha' ? (
      <HStack gap={10}>
        <Text tone="secondary" grow>{t(`conversa.arquivar.${estado.falha}`)}</Text>
        {estado.falha === 'hubAntigo' ? null : <Button kind="secondary" onClick={ler}>{t('conversa.arquivar.tentarDeNovo')}</Button>}
      </HStack>
    ) : (
      <ValueRow label={t('conversa.arquivar.prazo')} detail={t('conversa.arquivar.prazoDetalhe')}>
        {gravando ? <Spinner /> : null}
        <ChoicePicker language={superficieIdioma(idioma)} selection={estado.valor.days ?? NUNCA} disabled={gravando}
          options={opcoesDias(estado.valor.days).map((id) => ({ id, label: rotulo(id) }))}
          onChange={(id) => { if (id !== (estado.valor.days ?? NUNCA)) gravar(id) }} />
      </ValueRow>
    )
  return <CardSection title={t('conversa.arquivar.titulo')} footer={estado.kind === 'pronto' ? rodape : null}>{corpo}</CardSection>
}
