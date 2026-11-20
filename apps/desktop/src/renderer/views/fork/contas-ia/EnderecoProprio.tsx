import { useEffect, useRef, useState } from 'react'
import { Button, CardSection, Spinner } from '../../../components/Controls'
import { HStack, VStack } from '../../../components/Stack'
import { Text } from '../../../components/Text'
import { useStore } from '../../../store/context'
import { liveClient } from '../../settings/parts'
import { useT } from '../../../lib/fork-i18n/estado'
import { isolado, ltr } from '../../../../shared/bidi.ts'
import { MarketBadge } from '../../marketplace/pieces'
import type { ChaveTexto } from '../../../lib/fork-i18n/index.ts'
import { enderecoPronto, FORMATOS, removerChave, salvarEndereco, testarChave, type Acesso, type EnderecoProprio, type FormatoProprio, type NovoEndereco } from './api-direta'
import { ResultadoDoTeste } from './ApiDireta'
import { esquecerModelos } from '../seletor-modelo/catalogo'

const NOME_DO_FORMATO: Readonly<Record<FormatoProprio, ChaveTexto>> = {
  'chat-completions': 'contas.apiDireta.proprio.formato.chatCompletions',
  responses: 'contas.apiDireta.proprio.formato.responses',
  'anthropic-messages': 'contas.apiDireta.proprio.formato.anthropicMessages',
}

const VAZIO: NovoEndereco = { nome: '', url: '', formato: 'chat-completions', local: false, chave: '', modelo: '' }

type Resultado = Acesso | { ok: false; motivo: string }

/** API direta → Endereço próprio: um serviço num dos três formatos, com endereço e chave da pessoa. */
export function EnderecosProprios({ enderecos, aoMudar }: { enderecos: EnderecoProprio[]; aoMudar: () => Promise<void> }) {
  const t = useT()
  const [abrindo, setAbrindo] = useState(false)
  return (
    <CardSection title={t('contas.apiDireta.proprio.titulo')} footer={t('contas.apiDireta.proprio.rodape')}>
      {enderecos.map((e) => <LinhaDoEndereco key={e.id} endereco={e} aoMudar={aoMudar} />)}
      {abrindo ? (
        <NovoEnderecoForm aoFechar={() => setAbrindo(false)} aoMudar={aoMudar} />
      ) : (
        <button className="settings-quiet-button" onClick={() => setAbrindo(true)}>{t('contas.apiDireta.proprio.adicionar')}</button>
      )}
    </CardSection>
  )
}

function useRodar(aoMudar: () => Promise<void>) {
  const store = useStore()
  const [ocupado, setOcupado] = useState(false)
  const [resultado, setResultado] = useState<Resultado | null>(null)
  const ocupadoRef = useRef(false)
  const rodar = (trabalho: (client: NonNullable<ReturnType<typeof liveClient>>) => Promise<Acesso | { ok: boolean }>, depois?: (r: Acesso) => void) => {
    const client = liveClient(store)
    if (!client || ocupadoRef.current) return
    ocupadoRef.current = true
    setOcupado(true)
    setResultado(null)
    void trabalho(client)
      .then((r) => {
        if ('erro' in r || 'modelos' in r) {
          setResultado(r as Acesso)
          depois?.(r as Acesso)
        }
        esquecerModelos(store.computer.id, 'mexerica')
        return aoMudar()
      })
      .catch((e: unknown) => setResultado({ ok: false, motivo: e instanceof Error ? e.message : String(e) }))
      .finally(() => {
        ocupadoRef.current = false
        setOcupado(false)
      })
  }
  return { ocupado, resultado, rodar }
}

function LinhaDoEndereco({ endereco: e, aoMudar }: { endereco: EnderecoProprio; aoMudar: () => Promise<void> }) {
  const t = useT()
  const { ocupado, resultado, rodar } = useRodar(aoMudar)
  return (
    <VStack gap={8}>
      <div className="settings-row">
        <VStack grow className="settings-stack">
          <HStack gap={6}>
            <Text tone="text" dir="auto">{isolado(e.nome)}</Text>
            <MarketBadge label={t(NOME_DO_FORMATO[e.formato] as 'contas.apiDireta.proprio.formato.responses')} tone="secondary" />
            {e.local ? <MarketBadge label={t('contas.apiDireta.proprio.localSelo')} tone="secondary" /> : null}
          </HStack>
          <Text size="caption" tone="tertiary" truncate title={e.url}>{ltr(e.url)}</Text>
          {e.modelos ? <Text size="caption" tone="tertiary">{t.plural('contas.apiDireta.modelos', e.modelos.length)}</Text> : null}
        </VStack>
        {ocupado ? <Spinner /> : (
          <HStack gap={12}>
            <button className="settings-quiet-button" onClick={() => rodar((c) => testarChave(c, e.id))}>{t('contas.apiDireta.testar')}</button>
            <button className="settings-quiet-button" onClick={() => rodar((c) => removerChave(c, e.id))}>{t('contas.apiDireta.remover')}</button>
          </HStack>
        )}
      </div>
      {resultado ? <ResultadoDoTeste resultado={resultado} /> : null}
    </VStack>
  )
}

function NovoEnderecoForm({ aoFechar, aoMudar }: { aoFechar: () => void; aoMudar: () => Promise<void> }) {
  const t = useT()
  const [dados, setDados] = useState<NovoEndereco>(VAZIO)
  const { ocupado, resultado, rodar } = useRodar(aoMudar)
  // Fechar a tela no meio da digitação não deixa a chave em estado nenhum.
  useEffect(() => () => setDados(VAZIO), [])
  const mudar = <K extends keyof NovoEndereco>(campo: K, valor: NovoEndereco[K]) => setDados((d) => ({ ...d, [campo]: valor }))
  const salvar = () => {
    if (!enderecoPronto(dados)) return
    rodar((c) => salvarEndereco(c, dados, () => mudar('chave', '')), (r) => {
      if (r.ok) {
        setDados(VAZIO)
        aoFechar()
      }
    })
  }
  const campo = (rotulo: ChaveTexto, valor: string, aoMudar: (v: string) => void, tipo = 'text') => (
    <input
      type={tipo}
      aria-label={(t as unknown as (c: ChaveTexto) => string)(rotulo)}
      placeholder={(t as unknown as (c: ChaveTexto) => string)(rotulo)}
      value={valor}
      spellCheck={false}
      autoComplete="off"
      className="settings-key-input"
      dir={tipo === 'text' && rotulo === 'contas.apiDireta.proprio.nome' ? 'auto' : 'ltr'}
      onChange={(ev) => aoMudar(ev.target.value)}
    />
  )
  return (
    <VStack gap={8}>
      {campo('contas.apiDireta.proprio.nome', dados.nome, (v) => mudar('nome', v))}
      {campo('contas.apiDireta.proprio.url', dados.url, (v) => mudar('url', v))}
      <HStack gap={8} className="settings-row">
        <select aria-label={t('contas.apiDireta.proprio.formatoRotulo')} value={dados.formato} onChange={(ev) => mudar('formato', ev.target.value as FormatoProprio)}>
          {FORMATOS.map((f) => <option key={f} value={f}>{(t as unknown as (c: ChaveTexto) => string)(NOME_DO_FORMATO[f])}</option>)}
        </select>
        <label>
          <input type="checkbox" checked={dados.local} onChange={(ev) => mudar('local', ev.target.checked)} /> {t('contas.apiDireta.proprio.local')}
        </label>
      </HStack>
      {campo('contas.apiDireta.proprio.chave', dados.chave, (v) => mudar('chave', v), 'password')}
      {campo('contas.apiDireta.proprio.modelo', dados.modelo, (v) => mudar('modelo', v))}
      <Text size="caption" tone="tertiary">{t('contas.apiDireta.proprio.dica')}</Text>
      <HStack gap={8} className="settings-row">
        {ocupado ? <Spinner /> : <Button onClick={salvar} disabled={!enderecoPronto(dados)}>{t('contas.apiDireta.salvar')}</Button>}
        <button className="settings-quiet-button" onClick={() => { setDados(VAZIO); aoFechar() }}>{t('contas.apiDireta.cancelar')}</button>
      </HStack>
      {resultado ? <ResultadoDoTeste resultado={resultado} /> : null}
    </VStack>
  )
}
