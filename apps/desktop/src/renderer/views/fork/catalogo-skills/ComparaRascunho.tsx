import { useEffect, useMemo, useRef, useState, type KeyboardEvent, type ReactNode } from 'react'
import { Button, CardForm, CardSection, Disclosure, Spinner, ValueRow } from '../../../components/Controls'
import { Icon } from '../../../components/Icon'
import { ModalHeader, SheetPage } from '../../../components/Overlay'
import { HStack, VStack } from '../../../components/Stack'
import { Text } from '../../../components/Text'
import { useModel } from '../../../lib/observable'
import type { SkillReviewComparison, SkillReviewDocument } from '../../../../shared/fork-skill-review'
import type { CatalogoStore, IntencaoHub } from './catalogo-store'
import { curto, mensagemDeErro } from './regras'
import { MAX_DOCUMENTO, bytesDe, codigoDeRevisao, diffDeLinhas, pedidoDeRevisao, type CodigoRevisao } from './revisao'
import { textoDe, frase, type Frase } from './textos'
import { useTextosSkills } from './textos-tela'
import { formatarNumero } from '../../../lib/fork-i18n/index.ts'
import { superficieIdioma, useIdioma } from '../../../lib/fork-i18n/estado'


/** O alvo capturado no clique de "Comparar rascunho", na folha de detalhes de origem. */
export type SessaoRevisao = Readonly<{ origem: IntencaoHub; id: string; installationId: string; nome: string }>

/**
 * D2A-UI1: só o texto digitado e o alvo original da abertura, em memória da lista que abriu a folha.
 * Sobrevive à folha de detalhes recomeçar (queda/hello do mesmo Hub), não a Fechar/Voltar. Não é
 * permissão: não guarda base, resultado nem conexão; ler de novo exige gesto novo no mesmo alvo.
 */
export type RascunhoRetido = Readonly<{ hub: string; id: string; installationId: string; texto: string }>
export type RetencaoRascunho = Readonly<{ ler: () => RascunhoRetido | null; guardar: (r: RascunhoRetido | null) => void }>

/** A memória do texto para quem abre a folha (lista e matriz): `limpar` ao fechar ou abrir outra linha. */
export function useRetencaoRascunho() {
  const atual = useRef<RascunhoRetido | null>(null)
  return useMemo(() => ({
    retencao: { ler: () => atual.current, guardar: (r: RascunhoRetido | null) => { atual.current = r } } as RetencaoRascunho,
    limpar: () => { atual.current = null },
  }), [])
}

type Base = { doc: SkillReviewDocument; origem: IntencaoHub }
type Comparacao = { r: SkillReviewComparison; edicao: number; origem: IntencaoHub }
// Lição da R1 do #572: a mensagem fica como frase (ou nenhuma) e é traduzida ao mostrar.
type Falha = { acao: 'ler' | 'comparar'; codigo: CodigoRevisao | null; mensagem: Frase | null }
type Aba = 'diff' | 'base' | 'rascunho'

/** Recusas sem base para editar: a folha mostra só o motivo. */
const TERMINAIS = new Set<CodigoRevisao>(['skill_review_base_unknown', 'skill_review_manual_only', 'skill_installation_stale',
  'skill_installation_changed', 'skill_installation_unavailable', 'hub_sem_revisao'])

/**
 * d2a / DON-855: rascunho de uma skill escrita à mão comparado com a instalada, pelo Hub, sem efeito.
 * Nada é salvo, aprovado ou reservado; ao sair, o rascunho some.
 *
 * Cada pedido é preso ao alvo e à conexão capturados no gesto (`sessao`) e a um contador: resposta
 * de pedido anterior, de outro texto (mesmo que o texto volte igual) ou depois de troca/reconexão
 * não aparece. Reconexão ao mesmo Hub só volta a ler por gesto novo, com captura nova; outro Hub
 * nunca recebe o ID desta folha.
 */
export function ComparaRascunho({ loja, sessao, onVoltar, textoRetido, aoMudarTexto }: {
  loja: CatalogoStore
  sessao: SessaoRevisao
  onVoltar: () => void
  /** Reabertura depois de a folha recomeçar: o texto volta, sem base nem pedido até um gesto novo. */
  textoRetido?: string
  aoMudarTexto?: (texto: string) => void
}) {
  const t = useTextosSkills().revisao
  const idioma = useIdioma()
  const tx = (f: Frase) => textoDe(f, idioma)
  useModel(loja)
  const [base, setBase] = useState<Base | null>(null)
  const [rascunho, setRascunho] = useState<string | null>(textoRetido ?? null)
  /** Reaberta com o texto retido: nada é lido até "Ler a instalada de novo". */
  const [esperaGesto, setEsperaGesto] = useState(textoRetido !== undefined)
  const [edicao, setEdicao] = useState(0)
  const [comparacao, setComparacao] = useState<Comparacao | null>(null)
  const [andando, setAndando] = useState<'ler' | 'comparar' | null>(null)
  const [falha, setFalha] = useState<Falha | null>(null)
  /** A base vista ficou para trás (`skill_review_base_changed`): comparar espera uma releitura. */
  const [baseVencida, setBaseVencida] = useState(false)
  const [outroHub, setOutroHub] = useState(false)
  const [aba, setAba] = useState<Aba>('diff')
  const [tecnico, setTecnico] = useState(false)
  const token = useRef(0)
  const edicaoAtual = useRef(0)
  const viva = useRef(true)
  useEffect(() => {
    viva.current = true
    return () => { viva.current = false; token.current += 1 }
  }, [])

  const lerBase = async (origem: IntencaoHub) => {
    const meu = ++token.current
    setAndando('ler')
    setFalha(null)
    setComparacao(null)
    try {
      const r = await loja.revisar(sessao.id, pedidoDeRevisao(sessao.installationId), origem)
      if (!viva.current || meu !== token.current) return
      if (r === null) { setFalha({ acao: 'ler', codigo: null, mensagem: null }); return }
      setBase({ doc: r.base, origem })
      setBaseVencida(false)
      setEsperaGesto(false)
      // A primeira leitura abre o rascunho como cópia exata; releituras preservam o que foi escrito.
      setRascunho((atual) => atual ?? r.base.markdown)
    } catch (e) {
      if (viva.current && meu === token.current) setFalha({ acao: 'ler', codigo: codigoDeRevisao(e), mensagem: mensagemDeErro(e) })
    } finally {
      if (viva.current && meu === token.current) setAndando(null)
    }
  }

  // Abre a base na conexão capturada no clique; nunca na de agora. Reaberta: espera o gesto.
  useEffect(() => { if (textoRetido === undefined) void lerBase(sessao.origem) }, [])
  // Só o texto sai daqui (para sobreviver à folha recomeçar); base e resultado nunca.
  useEffect(() => { if (rascunho !== null) aoMudarTexto?.(rascunho) }, [rascunho])

  const conexaoDaBase = base !== null && loja.conexaoAtual(base.origem)
  const bytes = rascunho === null ? 0 : bytesDe(rascunho)
  const podeComparar = conexaoDaBase && !baseVencida && andando === null && rascunho !== null && rascunho.trim() !== '' && bytes <= MAX_DOCUMENTO

  const comparar = async () => {
    if (!podeComparar || base === null || rascunho === null) return
    const meu = ++token.current
    const enviada = edicaoAtual.current
    setAndando('comparar')
    setFalha(null)
    try {
      const pedido = pedidoDeRevisao(sessao.installationId, { expectedBodyDigest: base.doc.bodyDigest, markdown: rascunho })
      const r = await loja.revisar(sessao.id, pedido, base.origem)
      // Editar durante o pedido (mesmo voltando ao texto igual) troca a edição: a resposta não aparece.
      if (!viva.current || meu !== token.current || enviada !== edicaoAtual.current) return
      if (r === null) { setFalha({ acao: 'comparar', codigo: null, mensagem: null }); return }
      setComparacao({ r, edicao: enviada, origem: base.origem })
      setAba('diff')
    } catch (e) {
      if (!viva.current || meu !== token.current) return
      const codigo = codigoDeRevisao(e)
      if (codigo === 'skill_review_base_changed') setBaseVencida(true)
      setComparacao(null)
      setFalha({ acao: 'comparar', codigo, mensagem: mensagemDeErro(e) })
    } finally {
      if (viva.current && meu === token.current) setAndando(null)
    }
  }

  const editar = (texto: string) => {
    edicaoAtual.current += 1
    // Um pedido em andamento deixa de valer; o botão volta a servir.
    token.current += 1
    setAndando(null)
    setEdicao(edicaoAtual.current)
    setRascunho(texto)
  }

  /** Gesto novo: captura a conexão de agora, só se ainda for o Hub e a época desta folha. */
  const lerDeNovo = () => {
    const nova = loja.capturarIntencao()
    if (nova !== null && (nova.hub !== sessao.origem.hub || nova.epoca !== sessao.origem.epoca)) { setOutroHub(true); return }
    if (nova === null) { setFalha({ acao: 'ler', codigo: null, mensagem: null }); return }
    void lerBase(nova)
  }

  const tentar = () => {
    if (falha?.acao === 'comparar' && conexaoDaBase) void comparar()
    else lerDeNovo()
  }

  const valida = comparacao !== null && comparacao.edicao === edicao && loja.conexaoAtual(comparacao.origem) ? comparacao.r : null
  const desatualizada = comparacao !== null && valida === null && comparacao.edicao !== edicao
  const perdeuConexao = !outroHub && andando === null && (esperaGesto || (base !== null ? !conexaoDaBase : falha !== null && falha.codigo === null && !loja.conexaoAtual(sessao.origem)))
  const terminal = falha?.codigo != null && TERMINAIS.has(falha.codigo)
  const semConexaoAgora = loja.capturarIntencao()?.cliente == null

  return (
    <SheetPage className="cs-rev">
      <div className="idioma-superficie" {...superficieIdioma(idioma)} style={{ display: 'contents' }}>
      <ModalHeader
        closeTitle={tx(frase('skills.fechar'))}
        icon={<HStack gap={10}>
          <button type="button" className="cs-rev-voltar" onClick={onVoltar} aria-label={tx(frase('skills.revisao.voltarDe', { nome: sessao.nome }))}><Icon name="chevron.left" size={11} />{tx(frase('skills.revisao.detalhes'))}</button>
          <span className="modal-header-icon" aria-hidden><Icon name="book.pages" size={15} /></span>
        </HStack>}
        title={t.titulo}
        subtitle={<><bdi translate="no" dir="ltr">{sessao.id}</bdi> · {tx(frase('skills.revisao.escritaAMao'))}</>}
      />
      <CardForm>
        {outroHub ? (
          <Aviso tom="danger" icone="exclamationmark.triangle" titulo={t.outroHub} detalhe={t.outroHubDetalhe} />
        ) : perdeuConexao ? (
          <Aviso tom={semConexaoAgora ? undefined : 'danger'} icone={semConexaoAgora ? 'wifi.slash' : 'exclamationmark.triangle'}
            titulo={semConexaoAgora ? t.semConexao : t.conexaoMudou} detalhe={semConexaoAgora ? t.semConexaoDetalhe : t.conexaoMudouDetalhe}
            acao={<Button kind="secondary" disabled={semConexaoAgora} onClick={lerDeNovo}>{t.lerDeNovo}</Button>} />
        ) : terminal && falha?.codigo ? (
          <Aviso icone={falha.codigo === 'skill_review_base_unknown' || falha.codigo === 'hub_sem_revisao' || falha.codigo === 'skill_review_manual_only' ? 'info.circle' : 'exclamationmark.triangle'}
            tom={falha.codigo.startsWith('skill_installation') ? 'danger' : undefined} titulo={t.falha[falha.codigo][0]} detalhe={t.falha[falha.codigo][1]} />
        ) : (
          <Aviso icone="info.circle" titulo={t.semEfeito} detalhe={t.semEfeitoDetalhe} />
        )}

        {falha?.codigo === 'skill_review_base_changed' && !perdeuConexao && !outroHub ? (
          <Aviso tom="danger" icone="exclamationmark.triangle" titulo={t.falha.skill_review_base_changed[0]} detalhe={t.falha.skill_review_base_changed[1]}
            acao={<Button kind="secondary" disabled={andando !== null} onClick={lerDeNovo}>{t.lerDeNovo}</Button>} />
        ) : null}

        {base === null && andando === 'ler' ? (
          <CardSection footer={tx(frase('skills.revisao.copiaExata'))}><HStack gap={8}><Spinner /><Text tone="secondary">{t.lendo}</Text></HStack></CardSection>
        ) : null}

        {rascunho !== null && !terminal && !outroHub ? (
          <CardSection>
            <VStack gap={8}>
              <HStack gap={8} wrap>
                <Text size="callout" weight="semibold" tone="secondary">{t.rascunho}</Text>
                {desatualizada ? <span className="cs-rev-selo aviso">{t.alterado}</span> : <span className="cs-rev-selo">{t.rascunhoSelo}</span>}
              </HStack>
              <textarea className="cs-rev-doc selectable" value={rascunho} spellCheck={false} translate="no" rows={14} dir="auto"
                aria-label={tx(frase('skills.revisao.rascunhoDe', { id: sessao.id }))} aria-describedby="cs-rev-medida"
                onChange={(e) => editar(e.target.value)} />
              <div id="cs-rev-medida"><HStack gap={12} className="cs-rev-medida">
                <Text size="caption" tone="secondary">{t.nome(sessao.id)}</Text>
                <span className="cs-espaco" />
                <Text size="caption" tone={bytes > MAX_DOCUMENTO ? 'danger' : 'secondary'} tabular>{t.tamanho(formatarNumero(idioma, bytes / 1024, { maximumFractionDigits: 1, minimumFractionDigits: 1 }))}</Text>
              </HStack></div>
              {bytes > MAX_DOCUMENTO ? <Text size="footnote" tone="danger">{t.grandeDemais}</Text> : null}
              <HStack gap={10} wrap>
                <Button kind="primary" disabled={!podeComparar} onClick={() => { void comparar() }}>
                  {andando === 'comparar' ? <><Spinner /> {t.comparando}</> : t.comparar}
                </Button>
                <Text size="caption" tone="secondary">{t.compararNota}</Text>
              </HStack>
            </VStack>
          </CardSection>
        ) : null}

        {falha && !terminal && !outroHub && !perdeuConexao && falha.codigo !== 'skill_review_base_changed' ? (
          falha.codigo !== null ? (
            <Aviso tom={falha.codigo === 'skill_review_scope_unavailable' ? 'attention' : 'danger'} icone="exclamationmark.triangle"
              titulo={t.falha[falha.codigo][0]} detalhe={t.falha[falha.codigo][1]}
              acao={falha.codigo === 'skill_review_invalid' ? undefined : <Button kind="secondary" disabled={andando !== null} onClick={tentar}>{t.tentar}</Button>} />
          ) : falha.mensagem ? (
            <Aviso tom="danger" icone="exclamationmark.triangle" titulo={tx(frase(falha.acao === 'ler' ? 'skills.revisao.falhaLer' : 'skills.revisao.falhaComparar', { motivo: falha.mensagem }))}
              acao={<Button kind="secondary" disabled={andando !== null} onClick={tentar}>{t.tentar}</Button>} />
          ) : null
        ) : null}

        {desatualizada && !outroHub && !perdeuConexao ? <Aviso icone="arrow.left.arrow.right" titulo={t.compareDeNovo} detalhe={t.compareDeNovoDetalhe} /> : null}

        {valida && valida.proposal && !outroHub ? <Resultado r={valida} proposta={valida.proposal} aba={aba} setAba={setAba} tecnico={tecnico} setTecnico={setTecnico} /> : null}
      </CardForm>
      <div className="cs-rev-rodape">
        <Text size="caption" tone="secondary">{t.rodape}</Text>
        <span className="cs-espaco" />
        <Button kind="secondary" onClick={onVoltar}>{t.voltar}</Button>
      </div>
      </div>
    </SheetPage>
  )
}

function Aviso({ tom, icone, titulo, detalhe, acao }: { tom?: 'danger' | 'attention'; icone: string; titulo: string; detalhe?: string; acao?: ReactNode }) {
  return (
    <CardSection>
      <div className={`cs-rev-aviso${tom ? ` ${tom}` : ''}`} role={tom ? 'alert' : 'status'}>
        <Icon name={icone} size={13} className="cs-rev-aviso-icone" />
        <VStack gap={2} align="flex-start">
          <Text weight="semibold" tone={tom ?? 'text'} className="selectable">{titulo}</Text>
          {detalhe ? <Text tone="secondary" className="selectable">{detalhe}</Text> : null}
          {acao ? <div className="cs-rev-aviso-acao">{acao}</div> : null}
        </VStack>
      </div>
    </CardSection>
  )
}

function Resultado({ r, proposta, aba, setAba, tecnico, setTecnico }: {
  r: SkillReviewComparison; proposta: SkillReviewDocument; aba: Aba; setAba: (a: Aba) => void; tecnico: boolean; setTecnico: (f: (v: boolean) => boolean) => void
}) {
  const t = useTextosSkills().revisao
  const idioma = useIdioma()
  const tx = (f: Frase) => textoDe(f, idioma)
  const linhas = useMemo(() => diffDeLinhas(r.base.markdown, proposta.markdown), [r.base.markdown, proposta.markdown])
  const add = linhas.filter((l) => l.tipo === 'adicionada').length
  const del = linhas.filter((l) => l.tipo === 'removida').length
  const abas: { id: Aba; label: string }[] = [
    { id: 'diff', label: t.abas.diff }, { id: 'base', label: t.abas.base(r.base.revision) }, { id: 'rascunho', label: t.abas.rascunho(proposta.revision) },
  ]
  const refs = useRef<(HTMLButtonElement | null)[]>([])
  const teclas = (e: KeyboardEvent, i: number) => {
    const n = e.key === 'ArrowRight' ? (i + 1) % abas.length : e.key === 'ArrowLeft' ? (i + abas.length - 1) % abas.length
      : e.key === 'Home' ? 0 : e.key === 'End' ? abas.length - 1 : -1
    if (n < 0) return
    e.preventDefault()
    setAba(abas[n]!.id)
    refs.current[n]?.focus()
  }
  return (
    <>
      <CardSection footer={aba === 'diff' ? t.diffRodape : undefined}>
        <VStack gap={10}>
          <HStack gap={8}><Text size="callout" weight="semibold" tone="secondary">{t.comparacao}</Text><span className="cs-rev-selo ok">{t.lida}</span></HStack>
          <div className="tab-choice cs-rev-abas" role="tablist" aria-label={tx(frase('skills.revisao.verComparacao'))}>
            {abas.map((a, i) => (
              <button key={a.id} ref={(el) => { refs.current[i] = el }} type="button" role="tab" id={`cs-rev-aba-${a.id}`} aria-controls="cs-rev-painel"
                aria-selected={a.id === aba} tabIndex={a.id === aba ? 0 : -1} className={`tab-choice-option ${a.id === aba ? 'on' : ''}`}
                onClick={() => setAba(a.id)} onKeyDown={(e) => teclas(e, i)}>{a.label}</button>
            ))}
          </div>
          <div id="cs-rev-painel" role="tabpanel" aria-labelledby={`cs-rev-aba-${aba}`} tabIndex={0} translate="no">
            {aba === 'diff' ? (
              <VStack gap={8}>
                <HStack gap={10}><Text size="footnote" weight="semibold" tone="added">+{add}</Text><Text size="footnote" weight="semibold" tone="removed">−{del}</Text><Text size="footnote" tone="secondary">{t.resumo(linhas.length)}</Text></HStack>
                {/* Diff é código (R1 do #565): o painel vai da esquerda para a direita e isolado; cada linha na direção do próprio texto. */}
                <div className="cs-rev-diff selectable bidi-isolado" dir="ltr">
                  {linhas.map((l, i) => (
                    <div key={i} className={`cs-rev-ln ${l.tipo}`}>
                      <span className="cs-rev-n" aria-hidden>{l.antes ?? ''}</span>
                      <span className="cs-rev-n" aria-hidden>{l.depois ?? ''}</span>
                      <span className="cs-rev-s" aria-hidden>{l.tipo === 'adicionada' ? '+' : l.tipo === 'removida' ? '−' : ' '}</span>
                      <span className="cs-rev-t">{l.tipo === 'adicionada' ? <span className="cs-rev-sr">{tx(frase('skills.revisao.adicionada'))} </span> : l.tipo === 'removida' ? <span className="cs-rev-sr">{tx(frase('skills.revisao.removida'))} </span> : null}<bdi dir="auto">{l.texto || ' '}</bdi></span>
                    </div>
                  ))}
                </div>
              </VStack>
            ) : <pre className="cs-rev-inteiro selectable" dir="auto">{aba === 'base' ? r.base.markdown : proposta.markdown}</pre>}
          </div>
        </VStack>
      </CardSection>
      <CardSection title={tx(frase('skills.revisao.botsTitulo', { bots: t.bots, n: r.affectedBots.length }))} footer={t.botsRodape}>
        {r.affectedBots.length === 0 ? <Text tone="text">{t.nenhumBot}</Text> : (
          <ul className="cs-rev-bots">
            {r.affectedBots.map((b) => (
              <li key={b.id}><Icon name="person.crop.circle" size={16} className="cs-rev-rosto" /><Text tone="text" truncate translate="no" dir="auto" title={b.name}>{b.name || b.id}</Text></li>
            ))}
          </ul>
        )}
      </CardSection>
      <CardSection footer={tecnico ? t.tecnicoRodape : undefined}>
        <Disclosure expanded={tecnico} onToggle={() => setTecnico((v) => !v)} label={<Text tone="secondary">{t.tecnico}</Text>}>
          <VStack gap={4}>
            {/* Digests são código: da esquerda para a direita e isolados. */}
            <ValueRow label={tx(frase('skills.acao.instalada'))}><Text tone="secondary" mono className="selectable">{t.tecnicoInstalada(r.base.revision)} · <bdi dir="ltr" title={r.base.bodyDigest}>{curto(r.base.bodyDigest)}</bdi></Text></ValueRow>
            <ValueRow label={t.rascunho}><Text tone="secondary" mono className="selectable">{t.tecnicoRascunho(proposta.revision)} · <bdi dir="ltr" title={proposta.bodyDigest}>{curto(proposta.bodyDigest)}</bdi></Text></ValueRow>
            <ValueRow label={t.comparacao}><Text tone="secondary" mono className="selectable bidi-isolado" dir="ltr" title={r.comparisonDigest}>{curto(r.comparisonDigest)}</Text></ValueRow>
          </VStack>
        </Disclosure>
      </CardSection>
    </>
  )
}
