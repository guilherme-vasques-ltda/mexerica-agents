// DON-906: Ajustes → Conversa → Contexto. Superfície traduzida (DON-903), como a seção Idioma; o resto da página
// Conversa (C1) continua no legado até o lote dela.
// C2: o interruptor do medidor (preferência do aparelho).
// C3: "Comprimir em" e "Conversas paradas", ajuste do Hub selecionado (senão o deste computador), como Armazenamento:
// lido ao abrir e a cada conexão nova; gravado só ao escolher uma opção ou ao confirmar o campo (Enter ou sair dele),
// nunca a cada tecla. O Hub decide a autoridade; a UI só mostra a recusa. Nada no mirror (MIRROR_SCHEMA intocado).
import { useCallback, useEffect, useRef, useState, type ReactNode } from 'react'
import type { AppLanguage } from '../../../../shared/fork-idioma.ts'
import { CardSection, ChoicePicker, Spinner, ValueRow } from '../../../components/Controls'
import { superficieIdioma, useIdioma, useT, type Tradutor } from '../../../lib/fork-i18n/estado'
import { useModel } from '../../../lib/observable'
import { usePref } from '../../../lib/prefs'
import { useApp } from '../../../store/context'
import type { BotStore } from '../../../store/bot-store'
import { SwitchRow } from '../../bots/parts'
import { conversaPrefs } from '../conversa/preferencias'
import '../idioma/escritas.css'
import './contexto.css'
import { codigoFalha, conferirAjustes, falhaFixa, lerCampo, MINIMO_MIL, paraCampo, RPC_AJUSTES, RPC_SALVAR, type AjustesContexto, type ErroCampo, type Falha, type Parada } from './ajustes'
import { porcento, tokensCurtos } from './estado'

type Modo = 'auto' | 'manual'
type Estado = { kind: 'lendo' } | { kind: 'pronto'; ajustes: AjustesContexto } | { kind: 'falha'; falha: Falha }
/** O aviso do rodapé: erro do campo (nada enviado) ou falha do Hub. */
type Aviso = { campo: ErroCampo } | { falha: Falha }

/** A regra automática do Hub, só para o texto (o gatilho efetivo vem sempre do Hub). */
const AUTO_PCT = 75
const AUTO_MAXIMO = 200_000

const textoAviso = (t: Tradutor, idioma: AppLanguage, a: Aviso) =>
  'campo' in a
    ? a.campo === 'minimo' ? t('contexto.ajustes.erro.minimo', { min: tokensCurtos(idioma, MINIMO_MIL * 1000) }) : t(`contexto.ajustes.erro.${a.campo}`)
    : t(`contexto.ajustes.falha.${a.falha}`)

export function ContextoAjuste() {
  const idioma = useIdioma()
  const t = useT()
  const [ligado, ligar] = usePref(conversaPrefs.contextMeter)
  const app = useApp()
  const store = app.selectedStore ?? app.local
  const medidor = <SwitchRow title={t('contexto.mostrarMedidor')} detail={t('contexto.mostrarMedidor.detalhe')} on={ligado} onChange={ligar} />
  return (
    <div className="idioma-superficie" {...superficieIdioma(idioma)}>
      {store ? <Ajustes key={store.computer.id} source={store} medidor={medidor} /> : (
        <CardSection title={t('contexto.ajustes.secao')}>
          {medidor}
          <ValueRow label={t('contexto.ajustes.titulo')} detail={t('contexto.ajustes.semConexao')} />
        </CardSection>
      )}
    </div>
  )
}

function Ajustes({ source, medidor }: { source: BotStore; medidor: ReactNode }) {
  const idioma = useIdioma()
  const t = useT()
  const store = useModel(source)
  const client = store.connection.kind === 'online' ? store.connectedClient : null
  const origem = `${store.hello?.hostId ?? ''}|${store.principalEpoch}`
  const [estado, setEstado] = useState<Estado>({ kind: 'lendo' })
  const [salvando, setSalvando] = useState(false)
  const [aviso, setAviso] = useState<Aviso | null>(null)
  const [modo, setModo] = useState<Modo>('auto')
  const [campo, setCampo] = useState('')
  const vez = useRef(0)

  const mostrar = (a: AjustesContexto) => {
    setEstado({ kind: 'pronto', ajustes: a })
    setModo(a.triggerTokens === null ? 'auto' : 'manual')
    setCampo(a.triggerTokens === null ? '' : paraCampo(a.triggerTokens))
  }

  const ler = useCallback(() => {
    if (!client) return
    const minha = ++vez.current
    return client.call(RPC_AJUSTES, {}).then(
      (raw) => {
        if (vez.current !== minha) return
        const a = conferirAjustes(raw)
        if (a) mostrar(a)
        else setEstado({ kind: 'falha', falha: 'resposta' })
      },
      (e: unknown) => { if (vez.current === minha) setEstado({ kind: 'falha', falha: codigoFalha(e, 'ler') }) },
    )
  }, [client])

  // Outro Hub ou outra conexão: começa do zero e lê de novo.
  useEffect(() => { vez.current++; setEstado({ kind: 'lendo' }); setSalvando(false); setAviso(null) }, [origem])
  useEffect(() => { ler(); return () => { vez.current++ } }, [ler, origem])

  const salvar = (novo: AjustesContexto) => {
    if (!client || estado.kind !== 'pronto' || salvando) return
    const antes = estado.ajustes
    if (novo.triggerTokens === antes.triggerTokens && novo.idle === antes.idle) return
    const minha = vez.current
    setSalvando(true)
    setAviso(null)
    client.call(RPC_SALVAR, novo).then(
      (raw) => {
        if (vez.current !== minha) return
        setSalvando(false)
        // O Hub devolve o que gravou; se não devolver na forma, relê para mostrar o valor real.
        const a = conferirAjustes(raw)
        if (a) mostrar(a)
        else ler()
      },
      (e: unknown) => {
        if (vez.current !== minha) return
        // A falha pode chegar depois de o Hub gravar (rede): nada de voltar ao anterior como se fosse confirmado.
        // Relê o valor real e mantém o motivo no aviso; se a releitura falhar, a seção mostra a falha.
        // Controles travados até a releitura voltar: o valor na tela nunca passa por confirmado sem o Hub.
        setAviso({ falha: codigoFalha(e, 'salvar') })
        const relendo = ler()
        if (relendo) void relendo.finally(() => setSalvando(false))
        else setSalvando(false)
      },
    )
  }

  if (estado.kind !== 'pronto') {
    const detalhe = !client ? t('contexto.ajustes.semConexao') : estado.kind === 'lendo' ? null : t(`contexto.ajustes.falha.${estado.falha}`)
    return (
      <CardSection title={t('contexto.ajustes.secao')}>
        {medidor}
        <ValueRow label={t('contexto.ajustes.titulo')} detail={detalhe}>
          {client && estado.kind === 'lendo' ? <Spinner /> : null}
          {client && estado.kind === 'falha' && !falhaFixa(estado.falha) ? (
            <button type="button" className="styled-button secondary" onClick={() => { setEstado({ kind: 'lendo' }); ler() }}>{t('contexto.ajustes.tentarDeNovo')}</button>
          ) : null}
        </ValueRow>
      </CardSection>
    )
  }

  const { ajustes } = estado
  const soDono = !!aviso && 'falha' in aviso && aviso.falha === 'soDono'
  const travado = !client || salvando || soDono
  const confirmarCampo = () => {
    const r = lerCampo(campo)
    if ('erro' in r) { setAviso({ campo: r.erro }); return }
    salvar({ ...ajustes, triggerTokens: r.tokens })
  }
  const escolherModo = (m: Modo) => {
    setAviso(null)
    if (m === 'auto') { setModo('auto'); salvar({ ...ajustes, triggerTokens: null }); return }
    // Personalizado: abre o campo; só grava quando a pessoa confirmar um número.
    setModo('manual')
    setCampo(ajustes.triggerTokens === null ? '' : paraCampo(ajustes.triggerTokens))
  }

  return (
    <CardSection title={t('contexto.ajustes.secao')} footer={
      <span role="status" className={aviso ? 'ctx-ajuste-aviso' : undefined}>
        {aviso ? textoAviso(t, idioma, aviso) : t(salvando ? 'contexto.ajustes.salvando' : 'contexto.ajustes.rodape')}
      </span>
    }>
      {medidor}
      <ValueRow label={t('contexto.ajustes.comprimirEm')} detail={modo === 'auto'
        ? t('contexto.ajustes.automaticoDetalhe', { pct: porcento(idioma, AUTO_PCT), maximo: tokensCurtos(idioma, AUTO_MAXIMO) })
        : t('contexto.ajustes.personalizadoDetalhe', { min: tokensCurtos(idioma, MINIMO_MIL * 1000) })}>
        <span className="ctx-ajuste">
          {modo === 'manual' ? (
            <span className="ctx-ajuste-campo">
              <input aria-label={t('contexto.ajustes.campo')} inputMode="numeric" placeholder={paraCampo(150_000)} dir="ltr" value={campo} disabled={travado}
                aria-invalid={!!aviso && !soDono ? true : undefined}
                onChange={(e) => { setCampo(e.target.value); if (aviso) setAviso(null) }}
                onKeyDown={(e) => { if (e.key === 'Enter') { e.preventDefault(); confirmarCampo() } }}
                onBlur={() => { if (campo.trim()) confirmarCampo() }} />
              <span aria-hidden="true">{t('contexto.ajustes.unidade')}</span>
            </span>
          ) : null}
          <ChoicePicker<Modo> language={superficieIdioma(idioma)} selection={modo} disabled={travado} onChange={escolherModo}
            options={[{ id: 'auto', label: t('contexto.ajustes.automatico') }, { id: 'manual', label: t('contexto.ajustes.personalizado') }]} />
        </span>
      </ValueRow>
      <ValueRow label={t('contexto.ajustes.paradas')} detail={t(ajustes.idle === 'beforeExpiry' ? 'contexto.ajustes.antesDoCacheDetalhe' : 'contexto.ajustes.nuncaDetalhe')}>
        <ChoicePicker<Parada> language={superficieIdioma(idioma)} selection={ajustes.idle} disabled={travado} onChange={(idle) => salvar({ ...ajustes, idle })}
          options={[{ id: 'beforeExpiry', label: t('contexto.ajustes.antesDoCache') }, { id: 'never', label: t('contexto.ajustes.nunca') }]} />
      </ValueRow>
    </CardSection>
  )
}
