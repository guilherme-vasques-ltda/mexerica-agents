import { useLayoutEffect, useMemo, useRef, useState } from 'react'
import type { Entry } from '@shared/models'
import { Button } from '../../../components/Controls'
import { VStack } from '../../../components/Stack'
import { Text } from '../../../components/Text'
import { StoreContext, useStoreRef } from '../../../store/context'
import { useSelector } from '../../../lib/observable'
import { FormSheet } from '../../marketplace/pieces'
import { useCatalogo } from '../catalogo-skills/CatalogoSkills'
import { EscreverSkill } from '../catalogo-skills/EscreverSkill'
import { capturarExame, contextoDoRender, origemDoRender, prepararExame, somentePessoal, type ContextoRascunho } from './origem'
import { extrairProposta } from './rascunho'
import { IndicacaoSkill, InspecaoDaConversa } from './IndicacaoSkill'
import type { RascunhoSkill } from './rascunho'
import { RecomendacaoSkill } from './RecomendacaoSkill'
import { superficieIdioma, useIdioma, useT } from '../../../lib/fork-i18n/estado'
import './rascunho.css'

/** Montado só sob o Markdown da resposta final real; nunca interpreta conteúdo como intenção. */
export function RascunhoDaConversa({ entry, nomeAutor }: { entry: Entry; nomeAutor?: string }) {
  const t = useT()
  const idioma = useIdioma()
  const store = useStoreRef()
  // Re-render só quando muda a origem que o exame confere (conexão, época, Hub, espaço pessoal);
  // sem isso, uma reconexão sem evento nesta conversa deixaria `vista` velha e o exame falharia fechado.
  useSelector(store, (s) => s.hello?.hostId ?? null)
  useSelector(store, (s) => s.connectedClient)
  useSelector(store, (s) => s.principalEpoch)
  useSelector(store, (s) => s.spaces?.personalSpaceId ?? null)
  const loja = useCatalogo(store)
  const contextoAtual = useRef<ContextoRascunho | null>(null)
  const contexto = contextoDoRender(store, loja, entry.spaceId, contextoAtual.current)
  contextoAtual.current = contexto
  const texto = entry.data.text ?? ''
  const rascunho = useMemo(() => entry.kind === 'agent' && entry.data.final === true ? extrairProposta(texto) : null, [entry.id, entry.kind, entry.data.final, texto])
  const propostaAtual = useRef(rascunho)
  propostaAtual.current = rascunho
  // Só esta proposta nesta instância da UI; não grava nem promete persistência/sync.
  const chaveProposta = JSON.stringify([entry.id, rascunho])
  const [dispensadas, setDispensadas] = useState<ReadonlySet<string>>(() => new Set())
  const dispensadasAtuais = useRef(dispensadas)
  const vista = origemDoRender(store)
  const pessoal = somentePessoal(store, entry.spaceId)
  const entradaAtual = useRef(entry.id)
  entradaAtual.current = entry.id
  type Sessao = NonNullable<ReturnType<typeof capturarExame>> & { id: number; entrada: string; fonte: typeof store } & ({ tipo: 'indicacao'; reference: string } | { tipo: 'rascunho'; rascunho: RascunhoSkill })
  const [sessao, setSessao] = useState<Sessao | null>(null)
  const sessaoAberta = useRef(0)
  const ultimaSessao = useRef(0)
  const montada = useRef(true)
  useLayoutEffect(() => {
    montada.current = true
    return () => {
      montada.current = false
      sessaoAberta.current = 0
    }
  }, [])
  const ancora = useRef<HTMLDivElement>(null)
  const fechar = (id: number) => {
    if (id !== sessaoAberta.current) return
    sessaoAberta.current = 0
    setSessao(null)
  }
  const examinar = () => {
    // Outra instância após mudar o texto (inclusive A→B→A) não revive o gesto antigo.
    if (!montada.current || !rascunho || rascunho !== propostaAtual.current || (rascunho.tipo !== 'rascunho' && dispensadasAtuais.current.has(chaveProposta))) return
    const preparada = rascunho.tipo === 'indicacao' ? capturarExame(contexto, contextoAtual.current, vista, true) : prepararExame(contexto, contextoAtual.current, vista, rascunho)
    if (!preparada) return
    ancora.current?.querySelector('button')?.focus()
    const id = ++ultimaSessao.current
    sessaoAberta.current = id
    const corpo = rascunho.tipo === 'indicacao' ? { tipo: 'indicacao' as const, reference: rascunho.reference }
      : { tipo: 'rascunho' as const, rascunho: { name: rascunho.name, description: rascunho.description, instructions: rascunho.instructions } }
    setSessao({ ...preparada, fonte: store, id, entrada: entry.id, ...corpo })
  }
  const dispensar = () => {
    if (!montada.current || rascunho !== propostaAtual.current) return
    const novas = new Set(dispensadasAtuais.current).add(chaveProposta)
    dispensadasAtuais.current = novas
    setDispensadas(novas)
  }
  // Uma resposta alterada não desmonta nem substitui a sessão já examinada.
  if (!rascunho && !sessao) return null
  const aberta = (s: Sessao) => montada.current && sessaoAberta.current === s.id && entradaAtual.current === s.entrada
    && contextoAtual.current === s.contexto && s.contexto.spaceFonte === s.fonte.spaceId && somentePessoal(s.fonte, s.contexto.spaceEntrada)
  return <>
    {rascunho?.tipo === 'indicacao' ? (dispensadas.has(chaveProposta) ? null
      : <IndicacaoSkill key={chaveProposta} proposta={rascunho} nomeAutor={nomeAutor} pessoal={pessoal} ancoraExame={ancora} examinar={examinar} dispensar={dispensar} />) : rascunho?.tipo === 'recomendacao' ? (dispensadas.has(chaveProposta) ? null
      : <RecomendacaoSkill key={chaveProposta} proposta={rascunho} nomeAutor={nomeAutor} pessoal={pessoal} ancoraExame={ancora} examinar={examinar} dispensar={dispensar} />) : rascunho ? <VStack gap={8} align="flex-start" className="skill-draft">
      <Text size="callout" weight="semibold" translate="no">{rascunho.name}</Text>
      <Text size="footnote" tone="secondary">{t(pessoal ? 'conversa.skills.rascunho.revise' : 'conversa.skills.soPessoal')}</Text>
      <div ref={ancora}><Button kind="secondary" disabled={!pessoal} onClick={examinar}>{t('conversa.skills.examinarRascunho')}</Button></div>
    </VStack> : null}
    <FormSheet language={superficieIdioma(idioma)} width={sessao?.tipo === 'indicacao' ? 820 : 520} height={sessao?.tipo === 'indicacao' ? 700 : 600} open={sessao !== null} onClose={() => { if (sessao) fechar(sessao.id) }}>
      {sessao?.tipo === 'indicacao' ? <StoreContext.Provider value={sessao.fonte}>
        <InspecaoDaConversa key={sessao.id} referencia={sessao.reference} inspecao={{ origem: sessao.origem, valida: () => aberta(sessao) }} />
      </StoreContext.Provider> : sessao ? <EscreverSkill key={sessao.id} loja={sessao.loja} origem={sessao.origem} rascunhoInicial={sessao.rascunho}
        aberta={() => aberta(sessao)} onConcluir={() => fechar(sessao.id)} /> : null}
    </FormSheet>
  </>
}
