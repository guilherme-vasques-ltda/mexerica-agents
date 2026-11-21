// DON-907 C9 (D373; pedido do dono 06/11, seletor compacto, no estilo do t3code): o seletor de modelo, um componente
// só. No compositor (`conversa`) a escolha vale para a conversa; no criar bot (`padraoDoBot`) ele só devolve a
// escolha. Quem chama posiciona o painel e grava.
// Layout (Entrega 3 da revisão do app, proposta): trilho com nome (Favoritos, traço, provedores) | busca, conta do motor
// e lista; embaixo os ajustes do modelo escolhido (esforço e velocidade, só quando existem), a nota e o acesso.
// A busca olha todos os motores já lidos; modelo oculto na conta (Contas de IA) some, salvo o que está em uso.
// A lista é virtualizada (virtual.ts): só as linhas à vista vão para a tela, e o teclado anda pelo índice.
// Na função de fundo (`funcaoDeFundo`, Ajustes → Bot de fundo) é o mesmo painel, com conta e modelo numa escolha só; o que a
// função muda (motor recusado com motivo, conta que não serve, "Automático", rodapé) chega em `funcao` (ConfigFuncao).
// Textos: `conversa.seletor.*` (useT); os da função vêm prontos em `funcao`.
import { memo, useEffect, useId, useLayoutEffect, useMemo, useRef, useState, type KeyboardEvent } from 'react'
import { AgentIcon } from '../../../components/AgentIcon'
import { Icon } from '../../../components/Icon'
import { superficieIdioma, useIdioma, useT } from '../../../lib/fork-i18n/estado'
import { usePref } from '../../../lib/prefs'
import { isMac } from '../../settings/parts'
import { seletorPrefs } from './preferencias'
import { seloDoModelo } from './selo'
import {
  abaInicialDaFuncao, alternarFavorito, aplicarIntencao, atalho, chave, contaAlvo, contasDoMotor, destinoDaTroca, ehPadraoDoMotor, favoritosNoCatalogo, filtrarMemo, intencaoDeTroca,
  marcadaDaFuncao, modeloOculto, ocultosDoMotor, opcaoEsforco, tamanhoContexto, type CadastroContas, type ConfigFuncao, type MotivoTroca, type Escolha, type ModeloCatalogo, type ModoSeletor, type Provedor,
} from './regras'
import { ALTURA_LINHA, janela, LINHAS_A_VISTA, proximaLinha, rolagemPara } from './virtual'
import './seletor-modelo.css'

const FAVORITOS = 'favoritos'
const mod = isMac ? '⌘' : 'Ctrl+'

export interface AcessoSeletor {
  /** `ask` = Pedir; `auto` = Liberar. */
  nivel: string
  /** Abre onde o acesso muda, com a confirmação que já existe (configurações do bot). */
  mudar: () => void
}

/** Uma linha da lista, todas da mesma altura: "Padrão do motor", o nome de um motor (na busca) ou um modelo. */
type Item =
  | { tipo: 'padrao'; k: string; provedor: Provedor }
  | { tipo: 'grupo'; k: string; provedor: Provedor }
  /** `comMotor`: a linha diz o motor (Favoritos); `posicao`: a ordem entre os modelos da lista, a partir de 1. */
  | { tipo: 'modelo'; k: string; provedor: Provedor; modelo: ModeloCatalogo; atalho: number | null; oculto: boolean; comMotor: boolean; posicao: number }

/** O que uma linha faz; o objeto é sempre o mesmo, para a linha em memo não se redesenhar. */
interface Acoes { escolher: (provedor: Provedor, modelo: ModeloCatalogo) => void; favoritar: (backend: string, model: string) => void }

// A marca de "escolhido" é o visto à esquerda (como num menu); o fundo fica só para a linha sob o cursor ou o foco.
const marca = (on: boolean) => <span className="seletor-marca" aria-hidden="true">{on ? <Icon name="checkmark" size={11} weight="semibold" /> : null}</span>

/** Um modelo na lista. Em memo: digitar na busca ou rolar só redesenha as linhas que mudaram. */
const LinhaModelo = memo(function LinhaModelo({ indice, item, total, fav, escolhido, motivo, selo, idBase, acoes }: {
  indice: number
  item: Extract<Item, { tipo: 'modelo' }>
  total: number
  fav: boolean
  escolhido: boolean
  /** Por que a linha está desativada (já no idioma), ou `null`. */
  motivo: string | null
  /** Por qual conexão o modelo roda (já no idioma), ou `null` quando não é certo. */
  selo: string | null
  idBase: string
  acoes: Acoes
}) {
  const t = useT()
  const idioma = useIdioma()
  const { provedor, modelo, atalho: n, oculto: escondido } = item
  const contexto = tamanhoContexto(idioma, modelo.contextTokens)
  const dicaId = `${idBase}-ctx-${item.k}`
  return (
    <div className={`seletor-linha${escolhido ? ' on' : ''}`} data-i={indice} style={{ top: indice * ALTURA_LINHA }}>
      <button role="option" aria-selected={escolhido} aria-posinset={item.posicao} aria-setsize={total} className="seletor-modelo-nome" onClick={() => acoes.escolher(provedor, modelo)} disabled={!!motivo} title={motivo ?? (escondido ? t('conversa.seletor.ocultoEmUso') : undefined)}
        aria-label={`${modelo.name}, ${provedor.name}${selo ? `, ${selo}` : ''}${escondido ? `, ${t('conversa.seletor.oculto')}` : ''}`} aria-describedby={contexto ? dicaId : undefined}
        aria-keyshortcuts={n ? `${isMac ? 'Meta' : 'Control'}+${n}` : undefined}>
        {marca(escolhido)}
        <AgentIcon registry={provedor.registry ?? provedor.id} size={14} />
        {/* O nome vem do motor: a direção é a do próprio texto ("5.5 mini" não vira "mini 5.5" no árabe). */}
        <span className="seletor-nome" dir="auto">{modelo.name}</span>
        {escondido ? <span className="seletor-etiqueta"><Icon name="eye.slash" size={10} />{t('conversa.seletor.oculto')}</span> : null}
        <span className="seletor-meta">
          {item.comMotor && !selo ? <span>{provedor.name}</span> : null}
          {selo ? <span className="seletor-selo">{selo}</span> : null}
          {contexto ? <span id={dicaId}>{t('conversa.seletor.contexto', { tamanho: contexto })}</span> : null}
        </span>
      </button>
      <button className="seletor-estrela" aria-pressed={fav} aria-label={t(fav ? 'conversa.seletor.desfavoritar' : 'conversa.seletor.favoritar', { modelo: modelo.name })} title={t(fav ? 'conversa.seletor.desfavoritarDica' : 'conversa.seletor.favoritarDica')}
        onClick={() => acoes.favoritar(provedor.id, modelo.id)}>
        <Icon name={fav ? 'star.fill' : 'star'} size={12} />
      </button>
      <kbd className="seletor-atalho" aria-hidden="true">{n ? `${mod}${n}` : ''}</kbd>
    </div>
  )
})

/** Os motores com o mesmo conteúdo de antes devolvem o mesmo vetor: quem chama os refaz a cada desenho. */
function useMesmosProvedores(provedores: readonly Provedor[]): readonly Provedor[] {
  const guardado = useRef(provedores)
  const a = guardado.current
  const iguais = a === provedores || (a.length === provedores.length && a.every((p, i) => {
    const q = provedores[i]!
    return p.id === q.id && p.name === q.name && p.registry === q.registry && p.models === q.models && p.fastMode === q.fastMode && p.padrao === q.padrao && p.falhou === q.falhou
  }))
  // Só troca o guardado quando o conteúdo muda (o mesmo valor a cada desenho com as mesmas entradas).
  if (!iguais) guardado.current = provedores // eslint-disable-line react-hooks/refs
  return guardado.current // eslint-disable-line react-hooks/refs
}

export function SeletorModelo({ modo, provedores: provedoresDeFora, valor, carregar, onEscolher, acesso, onFechar, aviso, contas, tentarDeNovo, funcao }: {
  modo: ModoSeletor
  provedores: readonly Provedor[]
  valor: Escolha
  /** Pede o catálogo de um motor ainda não carregado. */
  carregar: (backend: string) => void
  onEscolher: (escolha: Escolha) => void
  acesso?: AcessoSeletor | null
  onFechar?: () => void
  /** Uma falha da última troca, já em texto do idioma (quem chama traduz o código, ex. `erroDaTroca`). */
  aviso?: string | null
  /**
   * D372 (R8), só no modo `conversa`: o cadastro de contas do Hub, para trocar a conversa de motor com a conta destino
   * explícita (`null` enquanto lê; `'falhou'` se a leitura falhou). Ausente, o seletor segue como antes (padrão do
   * bot e quem não a passa).
   */
  contas?: CadastroContas
  /** Lê de novo o catálogo de um motor cuja leitura falhou; ausente, a falha só mostra o texto. */
  tentarDeNovo?: (backend: string) => void
  /** Só no modo `funcaoDeFundo`: o que a função exige (ver `ConfigFuncao`); `contas` são as que servem a ela. */
  funcao?: ConfigFuncao
}) {
  const t = useT()
  const idioma = useIdioma()
  const [favoritos, setFavoritos] = usePref(seletorPrefs.favoritos)
  const f = modo === 'funcaoDeFundo' ? funcao ?? null : null
  const [aba, setAba] = useState<string>(() => (f ? abaInicialDaFuncao(valor.backend, provedoresDeFora, f.recusados) : favoritos.length ? FAVORITOS : valor.backend))
  const [busca, setBusca] = useState('')
  const lista = useRef<HTMLDivElement>(null)
  const bloco = useRef<HTMLDivElement>(null)
  const id = useId()
  const provedores = useMesmosProvedores(provedoresDeFora)

  // Favoritos lê os motores dos favoritos; uma aba de motor lê o dela; o esforço precisa do motor escolhido.
  const precisa = [...new Set([valor.backend, ...(aba === FAVORITOS ? favoritos.map((f) => f.split('/')[0]!) : [aba])])]
  useEffect(() => {
    for (const backend of precisa) if (provedores.find((p) => p.id === backend)?.models === null) carregar(backend)
  }, [precisa.join(','), provedores]) // eslint-disable-line react-hooks/exhaustive-deps

  // D372 (R8/N1): na conversa, outro motor só com a conta destino do cadastro: a única elegível vem marcada e visível
  // no campo Conta; com várias, nenhuma marcada e nada é enviado até a pessoa escolher; sem nenhuma, o motor fica
  // indisponível. O pedido só sai com a aba do motor e a conta já visíveis: um favorito ou atalho de outro motor
  // primeiro navega até lá (com o motivo, se houver). Nunca `accountId` vazio entre motores.
  // Entrega 3: no mesmo motor com várias contas, o campo Conta também aparece; marcar outra e escolher um modelo
  // leva a conversa para ela (a conta vai explícita no pedido).
  const [marcadas, setMarcadas] = useState<Record<string, string>>({})
  const [dica, setDica] = useState<string | null>(null)
  // O cadastro chegou (ou mudou): um motivo dado antes, como "carregando", deixa de valer; o campo Conta já mostra o atual.
  useEffect(() => setDica(null), [contas])
  const comContas = (modo === 'conversa' || !!f) && contas !== undefined
  const cadastro: CadastroContas = comContas ? (contas ?? null) : null
  // Na função de fundo não há "mesmo motor": o motor da escolha vira `''` para toda troca levar a conta explícita.
  const atualConta: Pick<Escolha, 'backend' | 'accountId'> = f ? { backend: '', accountId: null } : valor
  const marcada = (backend: string) => (f ? marcadaDaFuncao(valor, backend, marcadas) : marcadas[backend] ?? null)
  const emUso = (backend: string, model: string) => backend === valor.backend && model === valor.model
  const oculto = (backend: string, model: string) => modeloOculto(atualConta, backend, model, cadastro, marcada(backend))
  const visivel = (backend: string, model: string) => !oculto(backend, model) || emUso(backend, model)

  const atual = provedores.find((p) => p.id === valor.backend)?.models?.find((m) => m.id === valor.model) ?? null
  // Oculto sai também dos favoritos e dos atalhos (⌘1–⌘3 seguem os três primeiros que sobram).
  const favVisiveis = useMemo(() => favoritos.filter((k) => { const i = k.indexOf('/'); return visivel(k.slice(0, i), k.slice(i + 1)) }),
    [favoritos, cadastro, marcadas, valor.backend, valor.model, valor.accountId]) // eslint-disable-line react-hooks/exhaustive-deps
  const q = busca.trim()
  const provedorAba = aba === FAVORITOS ? undefined : provedores.find((p) => p.id === aba)
  const carregando = provedorAba?.models === null
  // A lista inteira, linha a linha (todas de 32px): o oculto é resolvido uma vez por motor e o filtro vem do memo por motor.
  const { itens, total, temOculto } = useMemo(() => {
    const noCatalogo = favoritosNoCatalogo(favVisiveis, provedores)
    const atalhos = new Map(noCatalogo.map((f) => [chave(f.provedor.id, f.modelo.id), f.atalho]))
    const itens: Item[] = []
    let total = 0
    let temOculto = false
    const modelo = (provedor: Provedor, m: ModeloCatalogo, escondido: boolean, comMotor: boolean) => {
      const k = chave(provedor.id, m.id)
      temOculto ||= escondido
      itens.push({ tipo: 'modelo', k, provedor, modelo: m, atalho: atalhos.get(k) ?? null, oculto: escondido, comMotor, posicao: ++total })
    }
    if (aba === FAVORITOS) {
      for (const f of noCatalogo) if (!q || filtrarMemo(f.provedor.models ?? [], q).includes(f.modelo)) modelo(f.provedor, f.modelo, oculto(f.provedor.id, f.modelo.id), true)
      return { itens, total, temOculto }
    }
    if (provedorAba && !carregando && !q && (!f || f.automatico?.backend === provedorAba.id)) itens.push({ tipo: 'padrao', k: `padrao/${provedorAba.id}`, provedor: provedorAba })
    // Com texto na busca, os outros motores já lidos entram abaixo, cada um com o nome (nenhuma leitura nova).
    for (const p of [...(provedorAba ? [provedorAba] : []), ...(q ? provedores.filter((o) => o.id !== aba && o.models?.length) : [])]) {
      const some = ocultosDoMotor(atualConta, p.id, cadastro, marcada(p.id))
      const linhas = filtrarMemo(p.models ?? [], q).flatMap((m) => { const escondido = some(m.id); return escondido && !emUso(p.id, m.id) ? [] : [{ m, escondido }] })
      if (q && linhas.length) itens.push({ tipo: 'grupo', k: `grupo/${p.id}`, provedor: p })
      for (const l of linhas) modelo(p, l.m, l.escondido, false)
    }
    return { itens, total, temOculto }
  }, [aba, q, favVisiveis, provedores, cadastro, marcadas, valor.backend, valor.model, valor.accountId]) // eslint-disable-line react-hooks/exhaustive-deps
  const favoritosSet = useMemo(() => new Set(favoritos), [favoritos])
  const nomeAba = aba === FAVORITOS ? t('conversa.seletor.favoritos') : (provedorAba?.name ?? aba)

  const destinoDe = (backend: string) => (comContas ? destinoDaTroca(atualConta.backend, backend, contas, marcada(backend)) : null)
  const intencao = (provedor: Provedor, model: string | null, emAba = aba) =>
    comContas ? intencaoDeTroca({ atual: atualConta, aba: emAba, provedor, model, contas: contas ?? null, marcada: marcada(provedor.id) }) : null
  const texto = (m: MotivoTroca | null) =>
    !m ? null : f && m.chave === 'conversa.seletor.conta.nenhuma' ? f.semConta(m.motor) : m.chave === 'conversa.seletor.conta.automaticoFora' ? (f?.automaticoFora(m.conta) ?? null) : 'conta' in m ? t(m.chave, { conta: m.conta }) : 'motor' in m ? t(m.chave, { motor: m.motor }) : t(m.chave)
  // Desativado só na aba do próprio motor, onde a conta e o motivo estão à vista; no favorito, o clique navega e explica.
  // Calculado uma vez por linha (e só das linhas que aparecem ou que o teclado visita), até a aba, a conta ou a conversa mudarem.
  const seloDe = (provedor: Provedor, modelo: string) => {
    const selo = seloDoModelo({ backend: provedor.id, modelo, nomeDoMotor: provedor.name, cobranca: contaAlvo(atualConta, provedor.id, cadastro, marcada(provedor.id))?.billing })
    if (!selo) return null
    return selo.grupo === null ? selo.nome : t(selo.grupo === 'planos' ? 'conversa.seletor.selo.plano' : 'conversa.seletor.selo.chave', { nome: selo.nome })
  }
  const motivos = useMemo(() => new Map<string, string | null>(), [aba, contas, marcadas, valor.backend, valor.accountId, t]) // eslint-disable-line react-hooks/exhaustive-deps
  const motivoNaAba = (provedor: Provedor, model: string | null) => {
    if (aba !== provedor.id) return null
    // O motor que a função não aceita: todas as linhas apagadas, com o motivo inteiro.
    if (f?.recusados[provedor.id]) return f.recusados[provedor.id]!.motivo
    const k = model ?? ''
    let motivo = motivos.get(k)
    if (motivo === undefined) {
      const i = intencao(provedor, model)
      motivos.set(k, motivo = i?.tipo === 'esperar' && (i.motivo.chave !== 'conversa.seletor.conta.escolha') ? texto(i.motivo) : null)
    }
    return motivo
  }
  const trocar = (provedor: Provedor, model: string | null) => {
    // Favorito ou atalho de um motor recusado: só leva até a aba dele, onde o motivo está inteiro.
    const recusado = f?.recusados[provedor.id]
    if (recusado) { setAba(provedor.id); setDica(recusado.motivo); return }
    const mesmo = provedor.id === valor.backend
    const direta = (): Escolha => ({ ...valor, backend: provedor.id, model, accountId: mesmo ? valor.accountId : null, modelOptions: {}, fastMode: mesmo && provedor.fastMode ? valor.fastMode : false })
    aplicarIntencao(intencao(provedor, model) ?? { tipo: 'mesmo' }, provedor.id, direta, {
      navegar: (motor, motivo) => { setAba(motor); setDica(texto(motivo)) },
      avisar: (motivo) => setDica(texto(motivo)),
      emitir: (escolha) => { setDica(null); onEscolher(escolha) },
    })
  }
  const escolherModelo = (provedor: Provedor, modelo: ModeloCatalogo) => trocar(provedor, modelo.id)
  // As linhas em memo recebem sempre o mesmo objeto; ele chama as funções do desenho mais recente.
  const agora = useRef<Acoes>(null)
  useLayoutEffect(() => { agora.current = { escolher: escolherModelo, favoritar: (backend, model) => setFavoritos(alternarFavorito(favoritos, backend, model)) } })
  const acoes = useMemo<Acoes>(() => ({ escolher: (p, m) => agora.current?.escolher(p, m), favoritar: (b, m) => agora.current?.favoritar(b, m) }), [])

  // Lista virtualizada: `primeira` é a linha no topo da área visível; a linha com foco fica na tela mesmo fora da
  // janela (rolar com o mouse não derruba o foco nem o teclado); `irPara` é a linha que o teclado pediu.
  const [rolado, setRolado] = useState(0)
  const [focada, setFocada] = useState<string | null>(null)
  const irPara = useRef<string | null>(null)
  const aoRolar = () => { if (lista.current) setRolado(Math.max(0, lista.current.scrollTop - (bloco.current?.offsetTop ?? 0))) }
  // Outra aba ou outra busca: a lista volta ao começo. Sem busca, se o modelo em uso está na lista e ficaria fora da
  // área visível, a lista já abre rolada o mínimo para mostrá-lo (ao abrir o seletor e ao chegar o catálogo do motor).
  const iEmUso = q ? -1 : itens.findIndex((x) => x.tipo === 'modelo' && emUso(x.provedor.id, x.modelo.id))
  useLayoutEffect(() => {
    const area = lista.current
    if (!area) return
    area.scrollTop = iEmUso < 0 ? 0 : rolagemPara(iEmUso, 0, area.clientHeight || LINHAS_A_VISTA * ALTURA_LINHA, bloco.current?.offsetTop ?? 0)
    aoRolar()
  }, [aba, q, iEmUso >= 0]) // eslint-disable-line react-hooks/exhaustive-deps
  const { inicio, fim } = janela(rolado, itens.length)
  const iFocada = focada === null ? -1 : itens.findIndex((x) => x.k === focada)
  const desenhadas = [...(iFocada >= 0 && iFocada < inicio ? [iFocada] : []), ...Array.from({ length: fim - inicio }, (_, n) => inicio + n), ...(iFocada >= fim ? [iFocada] : [])]
  useLayoutEffect(() => {
    if (irPara.current === null) return
    const alvo = itens.findIndex((x) => x.k === irPara.current)
    irPara.current = null
    bloco.current?.querySelector<HTMLElement>(`[data-i="${alvo}"] [role="option"]`)?.focus({ preventScroll: true })
  })
  const recebeFoco = (i: number) => { const x = itens[i]!; return x.tipo !== 'grupo' && !motivoNaAba(x.provedor, x.tipo === 'modelo' ? x.modelo.id : null) }
  // "Padrão do motor": modelo vazio, o mesmo que o "Default (recommended)" do motor, que sai da lista.
  const escolherPadrao = (provedor: Provedor) => trocar(provedor, null)
  const destinoAba = provedorAba ? destinoDe(provedorAba.id) : null
  // O campo Conta da aba: as contas conectadas do motor e a que a lista está mostrando.
  const contasAba = provedorAba && Array.isArray(cadastro) ? contasDoMotor(cadastro, provedorAba.id) : []
  const alvoAba = provedorAba ? contaAlvo(atualConta, provedorAba.id, cadastro, marcada(provedorAba.id)) : null
  const inativasAba = f && provedorAba ? f.inativas.filter((c) => c.backend === provedorAba.id) : []
  const recusadoAba = f && provedorAba ? f.recusados[provedorAba.id] : undefined
  const mudaConta = !!provedorAba && provedorAba.id === valor.backend && !!alvoAba && alvoAba.id !== (valor.accountId ?? null)
  const padraoEscolhido = !!provedorAba && provedorAba.id === valor.backend && ehPadraoDoMotor(valor, provedorAba)
  const esforco = opcaoEsforco(atual)
  const esforcoAtual = esforco ? (valor.modelOptions[esforco.id] ?? esforco.default ?? null) : null
  const rapido = provedores.find((p) => p.id === valor.backend)?.fastMode === true
  const nomeAtual = atual?.name ?? valor.model ?? t('conversa.seletor.padraoMotor')
  // As contas marcáveis do motor da aba (a mesma peça no chat e na função de fundo).
  const opcoesDeConta = provedorAba ? contasAba.map((c) => {
    const on = alvoAba?.id === c.id
    const daConversa = provedorAba.id === valor.backend && c.id === valor.accountId
    return (
      <button key={c.id} role="radio" aria-checked={on} className={`seletor-conta-opcao${on ? ' on' : ''}`}
        onClick={() => { setMarcadas({ ...marcadas, [provedorAba.id]: c.id }); setDica(null) }}>
        <bdi dir="auto">{c.label}</bdi>
        {daConversa ? <span className="seletor-conta-uso">{t('conversa.seletor.conta.emUso')}</span> : null}
      </button>
    )
  }) : null

  const onKey = (e: KeyboardEvent) => {
    if ((isMac ? e.metaKey : e.ctrlKey) && ['1', '2', '3'].includes(e.key)) {
      const alvo = atalho(favVisiveis, provedores, Number(e.key))
      e.preventDefault()
      // Na conversa, a tecla segurada (repetição automática) não é uma nova intenção: não emite de novo.
      if (comContas && e.repeat) return
      if (alvo) escolherModelo(alvo.provedor, alvo.modelo)
      return
    }
    if (e.key === 'Escape' && onFechar) { e.preventDefault(); e.stopPropagation(); onFechar(); return }
    if (e.key !== 'ArrowDown' && e.key !== 'ArrowUp') return
    const alvo = e.target as HTMLElement
    // Num grupo de rádios (conta, esforço, velocidade), as setas andam entre as opções dele; Enter/Espaço escolhe.
    const grupo = alvo.closest<HTMLElement>('[role="radiogroup"]')
    if (grupo) {
      const radios = [...grupo.querySelectorAll<HTMLElement>('[role="radio"]')]
      const at = radios.indexOf(alvo)
      e.preventDefault()
      radios[(at + (e.key === 'ArrowDown' ? 1 : -1) + radios.length) % radios.length]?.focus()
      return
    }
    // Na lista, só a partir da busca ou de uma linha dela. O próximo sai do índice (a linha pode nem estar na tela):
    // a lista rola o mínimo para mostrá-lo e o foco chega depois do desenho.
    if (!(alvo.matches('.seletor-busca') || lista.current?.contains(alvo))) return
    const de = Number(alvo.closest<HTMLElement>('[data-i]')?.dataset.i ?? -1)
    const para = proximaLinha(de, e.key === 'ArrowDown' ? 1 : -1, itens.length, recebeFoco)
    if (para < 0) return
    e.preventDefault()
    const area = lista.current!
    area.scrollTop = rolagemPara(para, area.scrollTop, area.clientHeight, bloco.current?.offsetTop ?? 0)
    irPara.current = itens[para]!.k
    setFocada(itens[para]!.k)
    aoRolar()
  }

  const nivel = (a: AcessoSeletor) => t(a.nivel === 'auto' ? 'conversa.seletor.acessoLiberar' : 'conversa.seletor.acessoPedir')
  const trilho = (p: { id: string; name: string; registry?: string }) => {
    const fora = f?.recusados[p.id]
    return (
      <button key={p.id} className={`seletor-trilho-item${aba === p.id ? ' on' : ''}${fora ? ' fora' : ''}`} aria-pressed={aba === p.id} aria-label={fora ? `${p.name}, ${fora.curto}` : p.name} title={fora?.motivo ?? p.name} onClick={() => { setAba(p.id); setDica(null) }}>
        <span className="seletor-trilho-icone" aria-hidden="true">{p.id === FAVORITOS ? <Icon name="star.fill" size={13} /> : <AgentIcon registry={p.registry ?? p.id} size={16} />}</span>
        {fora ? <span className="seletor-trilho-quem"><span className="seletor-trilho-nome">{p.name}</span><small>{fora.curto}</small></span> : <span className="seletor-trilho-nome">{p.name}</span>}
      </button>
    )
  }
  const vazio = carregando ? t('conversa.seletor.carregando')
    : total ? null
    : q ? t('conversa.seletor.semResultado')
    : aba === FAVORITOS ? t('conversa.seletor.semFavoritos')
    : provedorAba?.falhou ? null
    : provedorAba?.models?.length ? t('conversa.seletor.todosOcultos')
    : t('conversa.seletor.semModelos')

  const avisos = (
    <>
      {recusadoAba ? <div className="seletor-falha" role="note"><Icon name="exclamationmark.triangle" size={14} /><span>{recusadoAba.motivo}</span></div> : null}
      {!carregando && provedorAba?.falhou && !total ? (
        <div className="seletor-falha" role="alert">
          <Icon name="exclamationmark.triangle" size={14} />
          <span>{t('conversa.seletor.falhou', { motor: provedorAba.name })}</span>
          {tentarDeNovo ? <button className="seletor-seg on" onClick={() => tentarDeNovo(provedorAba.id)}>{t('conversa.seletor.tentarDeNovo')}</button> : null}
        </div>
      ) : null}
      {vazio ? <p className="seletor-vazio">{vazio}</p> : null}
    </>
  )

  return (
    <div className="seletor-modelo idioma-superficie" {...superficieIdioma(idioma)} role="dialog" aria-label={t('conversa.seletor.titulo')} onKeyDown={onKey}>
      <div className="seletor-colunas">
        <nav className="seletor-trilho" aria-label={t('conversa.seletor.provedores')}>
          {trilho({ id: FAVORITOS, name: t('conversa.seletor.favoritos') })}
          <span className="seletor-traco" aria-hidden="true" />
          {provedores.map(trilho)}
        </nav>
        <div className="seletor-meio">
          <input className="seletor-busca" type="search" placeholder={t('conversa.seletor.busca')} aria-label={t('conversa.seletor.busca')} aria-controls={`${id}-lista`} value={busca} onChange={(e) => setBusca(e.target.value)} autoFocus />
          {/* A vaga da conta tem altura fixa e existe sempre, para a lista não andar. Com texto na busca a lista mistura
              motores: a conta de um só confundiria, então a vaga fica vazia. */}
          {q ? <div className="seletor-conta" aria-hidden="true" /> : provedorAba && inativasAba.length ? (
            // Função de fundo: as contas que servem (marcáveis) e, apagadas, as que não servem agora, com o estado e a ação.
            <div className="seletor-conta seletor-conta-funcao">
              <div className="seletor-conta-grupo" role="radiogroup" aria-label={t('conversa.seletor.conta.grupo', { motor: provedorAba.name })}>
                <span className="seletor-conta-rotulo" aria-hidden="true">{t('conversa.seletor.conta.rotulo')}</span>
                {opcoesDeConta}
              </div>
              {inativasAba.map((c) => (
                <div key={c.id} className="seletor-conta-inativa">
                  <span className="seletor-conta-ponto" aria-hidden="true" />
                  <bdi dir="auto" className="seletor-conta-nome">{c.label}</bdi>
                  <span className="seletor-conta-estado">· {c.estado}</span>
                  {c.acao ? <button className="seletor-conta-acao" onClick={c.acao.fazer}>{c.acao.rotulo}</button> : null}
                </div>
              ))}
            </div>
          ) : provedorAba && destinoAba && (destinoAba.tipo === 'falhou' || destinoAba.tipo === 'carregando' || destinoAba.tipo === 'indisponivel') ? (
            <p className="seletor-conta" role="note" title={t(destinoAba.tipo === 'falhou' ? 'conversa.seletor.conta.falhou' : destinoAba.tipo === 'carregando' ? 'conversa.seletor.conta.carregando' : 'conversa.seletor.conta.nenhuma', { motor: provedorAba.name })}>{t(destinoAba.tipo === 'falhou' ? 'conversa.seletor.conta.falhou' : destinoAba.tipo === 'carregando' ? 'conversa.seletor.conta.carregando' : 'conversa.seletor.conta.nenhuma', { motor: provedorAba.name })}</p>
          ) : provedorAba && contasAba.length > 1 ? (
            <div className="seletor-conta" role="radiogroup" aria-label={t('conversa.seletor.conta.grupo', { motor: provedorAba.name })}>
              <span className="seletor-conta-rotulo" aria-hidden="true">{t('conversa.seletor.conta.rotulo')}</span>
              {opcoesDeConta}
            </div>
          ) : provedorAba && (alvoAba ?? contasAba[0]) ? (
            <p className="seletor-conta" role="note">
              <span className="seletor-conta-rotulo">{t('conversa.seletor.conta.rotulo')}</span>
              <bdi dir="auto" className="seletor-conta-unica">{(alvoAba ?? contasAba[0])!.label}</bdi>
            </p>
          ) : <div className="seletor-conta" aria-hidden="true" />}
          <div ref={lista} id={`${id}-lista`} className="seletor-lista" role="listbox" aria-label={aba === FAVORITOS ? nomeAba : t('conversa.seletor.modelosDe', { motor: nomeAba })} onScroll={aoRolar}>
            {/* Os avisos vêm depois de "Padrão do motor" (que é a primeira linha do bloco) e antes dos motores da busca. */}
            {itens[0]?.tipo === 'padrao' ? null : avisos}
            <div ref={bloco} className="seletor-janela" role="presentation" style={{ height: itens.length * ALTURA_LINHA }}
              onFocus={(e) => setFocada(itens[Number((e.target as HTMLElement).closest<HTMLElement>('[data-i]')?.dataset.i ?? -1)]?.k ?? null)}
              onBlur={(e) => { if (!e.currentTarget.contains(e.relatedTarget as Node | null)) setFocada(null) }}>
              {desenhadas.map((i) => {
                const item = itens[i]!
                if (item.tipo === 'grupo') return <div key={item.k} className="seletor-grupo-nome" aria-hidden="true" style={{ top: i * ALTURA_LINHA }}>{item.provedor.name}</div>
                if (item.tipo === 'padrao') {
                  const motivo = motivoNaAba(item.provedor, null)
                  return (
                    <div key={item.k} className={`seletor-linha${padraoEscolhido ? ' on' : ''}`} data-i={i} style={{ top: i * ALTURA_LINHA }}>
                      <button role="option" aria-selected={padraoEscolhido} className="seletor-modelo-nome" onClick={() => escolherPadrao(item.provedor)} disabled={!!motivo} title={motivo ?? undefined}
                        aria-label={`${f?.automatico?.nome ?? t('conversa.seletor.padraoMotor')}, ${item.provedor.name}`}>
                        {marca(padraoEscolhido)}
                        <AgentIcon registry={item.provedor.registry ?? item.provedor.id} size={14} />
                        <span className="seletor-nome">{f?.automatico?.nome ?? t('conversa.seletor.padraoMotor')}</span>
                        {!motivo && f?.automatico?.etiqueta ? <span className="seletor-etiqueta recomendado">{f.automatico.etiqueta}</span> : null}
                        {!motivo && f?.automatico?.meta ? <span className="seletor-meta">{f.automatico.meta}</span> : null}
                      </button>
                      <span className="seletor-estrela" aria-hidden="true" />
                      <kbd className="seletor-atalho" aria-hidden="true" />
                    </div>
                  )
                }
                return <LinhaModelo key={item.k} indice={i} item={item} total={total} fav={favoritosSet.has(item.k)} escolhido={emUso(item.provedor.id, item.modelo.id)} motivo={motivoNaAba(item.provedor, item.modelo.id)} selo={seloDe(item.provedor, item.modelo.id)} idBase={id} acoes={acoes} />
              })}
            </div>
            {itens[0]?.tipo === 'padrao' ? avisos : null}
            {temOculto ? <p className="seletor-linha-nota" role="note">{t('conversa.seletor.ocultoEmUso')}</p> : null}
          </div>
        </div>
      </div>
      {aviso ?? dica ? <p className="seletor-aviso" role="alert">{aviso ?? dica}</p> : null}
      {esforco || rapido ? (
        <div className="seletor-ajustes" role="group" aria-label={t('conversa.seletor.ajustesDe', { modelo: nomeAtual })}>
          <span className="seletor-ajustes-modelo" title={nomeAtual}>{nomeAtual}</span>
          {esforco ? (
            <div className="seletor-par" role="radiogroup" aria-label={t('conversa.seletor.esforcoDe', { modelo: nomeAtual })}>
              <span className="seletor-par-rotulo" aria-hidden="true">{esforco.name}</span>
              <div className="seletor-segs">
                {esforco.values.map((v) => (
                  <button key={v.id} role="radio" aria-checked={esforcoAtual === v.id} className={`seletor-seg${esforcoAtual === v.id ? ' on' : ''}`} title={esforco.default === v.id ? t('conversa.seletor.padrao') : undefined}
                    onClick={() => onEscolher({ ...valor, modelOptions: { ...valor.modelOptions, [esforco.id]: v.id } })}>{v.name}</button>
                ))}
              </div>
            </div>
          ) : null}
          {rapido ? (
            <div className="seletor-par" role="radiogroup" aria-label={t('conversa.seletor.velocidade')}>
              <span className="seletor-par-rotulo" aria-hidden="true">{t('conversa.seletor.velocidade')}</span>
              <div className="seletor-segs">
                {([['conversa.seletor.velocidadePadrao', false], ['conversa.seletor.velocidadeRapido', true]] as const).map(([chave, fast]) => (
                  <button key={chave} role="radio" aria-checked={valor.fastMode === fast} className={`seletor-seg${valor.fastMode === fast ? ' on' : ''}`} onClick={() => onEscolher({ ...valor, fastMode: fast })}>{t(chave)}</button>
                ))}
              </div>
            </div>
          ) : null}
        </div>
      ) : null}
      {modo === 'conversa' || f || acesso ? (
        <div className="seletor-rodape">
          {/* A troca de conta é dita aqui, no lugar da nota: o painel tem altura fixa e nada empurra a lista. */}
          {mudaConta && !q ? <span className="seletor-nota seletor-nota-troca" role="status">{f ? f.trocarDica(alvoAba!.label) : t('conversa.seletor.conta.trocarDica', { conta: alvoAba!.label })}</span>
            : f ? <span className="seletor-nota">{f.nota}</span> : modo === 'conversa' ? <span className="seletor-nota">{t('conversa.seletor.nota')}</span> : null}
          {acesso ? (
            <button className="seletor-acesso" onClick={acesso.mudar} aria-label={t('conversa.seletor.acessoRotulo', { nivel: nivel(acesso) })}>
              <Icon name={acesso.nivel === 'auto' ? 'checkmark.shield' : 'hand.raised'} size={12} />
              <span>{nivel(acesso)}</span>
              <Icon name="chevron.right" size={9} />
            </button>
          ) : null}
        </div>
      ) : null}
    </div>
  )
}
