import { useEffect, useRef, useState } from 'react'
import { Button, CardForm, CardSection, Spinner } from '../../components/Controls'
import { Icon } from '../../components/Icon'
import { ModalHeader, SheetPage } from '../../components/Overlay'
import { VStack } from '../../components/Stack'
import { Text } from '../../components/Text'
import { useStore } from '../../store/context'
import { useCatalogo } from '../fork/catalogo-skills/CatalogoSkills'
import { estadoDaFolha, lerInstalada, mesmaOrigem, origemDaFonte, type EstadoFolha, type LeituraFolha, type OrigemLeitura } from '../fork/catalogo-skills/consumidores'
import { codigoDeSkill, mensagemDeErro } from '../fork/catalogo-skills/regras'
import { TextoPassivo } from '../fork/catalogo-skills/TextoPassivo'
import { textoDe, frase, frases, juntas, type Frase } from '../fork/catalogo-skills/textos'
import { useTextosSkills } from '../fork/catalogo-skills/textos-tela'
import { superficieIdioma, useIdioma } from '../../lib/fork-i18n/estado'
import '../fork/catalogo-skills/catalogo-skills.css'

/**
 * The skill a details sheet shows: an installed one, opened by `id` from a bot's settings. `origem` is the
 * hub/link/generation captured in the click that opens it (D166 R10). `name`/`description` only identify the
 * row for the caller; the sheet never shows them.
 */
export type SkillRef = { id: string; name: string; description: string; origem: OrigemLeitura }

/**
 * A skill's details from a bot's settings (kit `SkillSheet`), read-only: the text the bot follows.
 * D166 R10: the whole presentation (title, description, text, Installed badge, footer) comes only from a
 * strong-protocol reply made on the origin of the gesture while that exact origin is still live; another
 * hub, a new connection or a protocol the Skills page has disproved hide all of it, with no fallback to
 * the opening row or an earlier read. Installing and removing live on the Skills page.
 */
export function SkillSheet({ skill }: { skill: SkillRef }) {
  const store = useStore()
  const textos = useTextosSkills()
  const idioma = useIdioma()
  const tx = (f: Frase) => textoDe(f, idioma)
  // The Skills page's own store: only its `semProtocolo` (fed by the strong wrapper) is read here.
  const loja = useCatalogo(store)
  // A caller that predates R10 (no `origem`) gets the origin at mount, the render right after its click.
  const [origem, setOrigem] = useState<OrigemLeitura>(() => (skill.origem !== undefined ? skill.origem : origemDaFonte(store)))
  const [tentativa, setTentativa] = useState(0)
  const [leitura, setLeitura] = useState<LeituraFolha | null>(null)
  const corpo = useRef<HTMLHeadingElement>(null)

  useEffect(() => {
    // One read per origin and attempt; nothing is asked outside the origin.
    if (!origem) return
    let live = true
    lerInstalada(store, origem, skill.id).then(
      (d) => { if (live && d) setLeitura({ origem, tentativa, resultado: { detalhes: d } }) },
      (e: unknown) => { if (live) setLeitura({ origem, tentativa, resultado: { erro: mensagemDeErro(e), codigo: codigoDeSkill(e) } }) },
    )
    return () => {
      live = false
    }
  }, [origem, tentativa])

  const semProtocoloNoHub = origem !== null && loja.semProtocolo && loja.capturarIntencao()?.hub === origem.hub
  // A read that stopped being valid is dropped in this very render, not kept until an effect runs.
  if (leitura !== null && (leitura.origem !== origem || semProtocoloNoHub || !mesmaOrigem(store, leitura.origem))) setLeitura(null)
  const estado = estadoDaFolha(store, origem, tentativa, leitura, semProtocoloNoHub)
  const detalhes = estado.tipo === 'pronta' ? estado.detalhes : null

  /** After a new read is asked for, focus stays inside the sheet instead of falling to the page behind. */
  const focarCorpo = () => setTimeout(() => corpo.current?.focus(), 0)
  /** A new gesture on the same hub: adopt today's connection (never another hub) and read once. */
  const lerDeNovo = async () => {
    if (semProtocoloNoHub) await loja.carregar()
    const agora = origemDaFonte(store)
    if (!agora || !origem || agora.hub !== origem.hub) return
    setOrigem(agora)
    setTentativa((n) => n + 1)
    focarCorpo()
  }
  const tentarDeNovo = () => {
    setTentativa((n) => n + 1)
    focarCorpo()
  }

  return (
    <SheetPage>
      <div className="idioma-superficie" {...superficieIdioma(idioma)} style={{ display: 'contents' }}>
      <ModalHeader
        closeTitle={tx(frase('skills.fechar'))}
        icon="book.pages"
        title={detalhes?.installation ? <bdi translate="no" dir="auto">{detalhes.installation.name}</bdi> : textos.folhaTitulo}
        trailing={detalhes ? (
          <span title={tx(frase('skills.acao.instalada'))} aria-label={tx(frase('skills.acao.instalada'))} className="skill-added">
            <Icon name="checkmark" size={13} weight="semibold" />
          </span>
        ) : undefined}
      />
      <CardForm>
        {/* Focus target after a new read, so focus never falls to the page behind the sheet. */}
        <h3 ref={corpo} tabIndex={-1} className="cs-titulo-oculto">{textos.folhaTitulo}</h3>
        {detalhes?.installation?.description ? (
          <CardSection>
            <Text size="subheadline" tone="secondary" className="selectable" translate="no" dir="auto">{detalhes.installation.description}</Text>
          </CardSection>
        ) : null}
        {estado.tipo === 'pronta' ? (
          <CardSection title={tx(frase('skills.instrucoes'))} footer={textos.folhaRodape}>
            {estado.detalhes.markdown ? <TextoPassivo texto={estado.detalhes.markdown} /> : <Text tone="secondary">{tx(frase('skills.detalhes.semInstrucoes'))}</Text>}
          </CardSection>
        ) : estado.tipo === 'lendo' ? (
          <CardSection title={tx(frase('skills.instrucoes'))}><Spinner /></CardSection>
        ) : (
          <CardSection>
            <Aviso estado={estado} onLerDeNovo={() => { void lerDeNovo() }} onTentarDeNovo={tentarDeNovo} lendo={loja.carregando} />
          </CardSection>
        )}
      </CardForm>
      </div>
    </SheetPage>
  )
}

/** Why nothing of the skill is shown, and the one honest next step. */
function Aviso({ estado, onLerDeNovo, onTentarDeNovo, lendo }: {
  estado: Exclude<EstadoFolha, { tipo: 'pronta' } | { tipo: 'lendo' }>
  onLerDeNovo: () => void
  onTentarDeNovo: () => void
  lendo: boolean
}) {
  const idioma = useIdioma()
  const [frase_, tom, acao]: [Frase, 'danger' | 'secondary', 'ler' | 'tentar' | 'semLink' | null] =
    estado.tipo === 'outroHub' ? [frases.folhaOutroHub, 'danger', null]
      : estado.tipo === 'conexaoMudou' ? (estado.semLink ? [frases.folhaSemLink, 'danger', 'semLink'] : [frases.folhaConexaoMudou, 'danger', 'ler'])
      : estado.tipo === 'semProtocolo' ? [juntas(frase('skills.semProtocoloFrase'), frases.semProtocoloDetalhe), 'secondary', 'ler']
      : estado.tipo === 'conteudo' ? [frases.conteudoInstrucoes[estado.status], 'secondary', 'ler']
      : [frases.folhaErro(estado.mensagem), 'danger', 'tentar']
  const mensagem = textoDe(frase_, idioma)
  return (
    <VStack gap={8} align="flex-start">
      <div role="status">
        <Text tone={tom} className="selectable">{mensagem}</Text>
      </div>
      {acao === 'ler' || acao === 'semLink' ? (
        <Button kind="secondary" disabled={acao === 'semLink' || lendo} onClick={onLerDeNovo}>{textoDe(frase('skills.detalhes.lerDeNovo'), idioma)}</Button>
      ) : acao === 'tentar' ? (
        <Button kind="secondary" onClick={onTentarDeNovo}>{textoDe(frase('skills.tentarDeNovo'), idioma)}</Button>
      ) : null}
    </VStack>
  )
}
