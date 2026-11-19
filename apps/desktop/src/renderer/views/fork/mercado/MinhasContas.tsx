// Telas 06 e 06b: Minhas contas. Renomear na própria linha, reconectar a conta que expirou e desconectar só uma, com
// o diálogo dizendo quais bots a usavam e, com honestidade, que só se apaga o acesso neste computador.
import { useEffect, useRef, useState } from 'react'
import { Button, DropdownMenu } from '../../../components/Controls'
import { Icon } from '../../../components/Icon'
import { Dialog } from '../../../components/Overlay'
import { HStack, VStack } from '../../../components/Stack'
import { Text } from '../../../components/Text'
import { superficieIdioma, useIdioma, useT } from '../../../lib/fork-i18n/estado'
import type { Mercado } from './estado'
import { agruparContas, botsQueUsam, contaExpirada, jeitoDaConta, jeitoRecomendado, podeConectar, type GrupoDeContas } from './regras.ts'
import type { Conta, Servico, TipoId } from './tipos.ts'
import { Aviso, Carregando, Erro, frasesDaRecusa, LogoServico, PontoDeEstado } from './pecas'

export function MinhasContas({ mercado, aoVerCatalogo, aoAdicionar }: {
  mercado: Mercado
  aoVerCatalogo: () => void
  /** Começa a conectar mais uma conta do mesmo serviço (a mesma tela de Conectar da ficha). */
  aoAdicionar: (servico: Servico) => void
}) {
  const t = useT()
  const idioma = useIdioma()
  const estado = mercado.estado
  const contas = mercado.contas
  const catalogo = mercado.catalogo
  const [renomeando, setRenomeando] = useState<string | null>(null)
  const [reconectando, setReconectando] = useState<string | null>(null)
  const [desconectando, setDesconectando] = useState<Conta | null>(null)
  const [avisos, setAvisos] = useState<string | null>(null)
  const [erro, setErro] = useState<string | null>(null)

  if (estado === 'lendo') return <div className="mercado"><Carregando texto={t('mercado.minhas.carregando')} /></div>
  if (estado === 'semConexao') return <div className="mercado"><Aviso>{t('mercado.catalogo.semConexao')}</Aviso></div>
  if (estado === 'erro' || mercado.contasErro !== null) {
    return (
      <div className="mercado">
        <VStack gap={8} className="mercado-vazio" align="center">
          <Text size="compactBody" weight="semibold" tone="text">{t('mercado.minhas.erro.titulo')}</Text>
          <Text size="callout" tone="secondary">{mercado.erro ?? (mercado.cofreTrancado ? t('mercado.cofre.trancado') : t('mercado.minhas.erro.texto'))}</Text>
          <Button kind="secondary" onClick={() => void mercado.ler()}>{t('mercado.catalogo.tentar')}</Button>
        </VStack>
      </div>
    )
  }
  if (contas.length === 0) {
    return (
      <div className="mercado">
        {avisos ? <Aviso alerta>{avisos}</Aviso> : null}
        <VStack gap={8} className="mercado-vazio" align="center">
          <Icon name="puzzlepiece.extension" size={30} color="var(--text-tertiary, var(--ink-3))" />
          <Text size="compactBody" weight="semibold" tone="text">{t('mercado.minhas.vazio.titulo')}</Text>
          <Text size="callout" tone="secondary">{t('mercado.minhas.vazio.texto')}</Text>
          <Button onClick={aoVerCatalogo}>{t('mercado.minhas.vazio.ver')}</Button>
        </VStack>
      </div>
    )
  }

  const reconectar = async (c: Conta) => {
    setReconectando(c.id); setErro(null)
    try { await mercado.reconectar(c.id) } catch (e) { setErro(frasesDaRecusa(t, e)) } finally { setReconectando(null) }
  }
  const nomeDoGrupo = (g: GrupoDeContas) => g.nome ?? t(g.foraDoCatalogo ? 'mercado.minhas.grupoOutro' : 'mercado.minhas.grupoMcp')
  /** O serviço do grupo, quando dá para conectar outra conta dele agora (fora do catálogo ou "Em breve" não dá). */
  const servicoParaAdicionar = (g: GrupoDeContas): Servico | null => {
    const s = g.servico === null ? undefined : catalogo?.servicos.find((x) => x.id === g.servico)
    return s && podeConectar(s) && jeitoRecomendado(s) !== null ? s : null
  }
  const nomeDoServico = (c: Conta) => catalogo?.servicos.find((s) => s.id === c.servico)?.nome ?? null
  const desconectar = async (c: Conta) => {
    setErro(null)
    try {
      const r = await mercado.desconectar(c.id)
      const servico = nomeDoServico(c)
      // O Hub diz `revogadoNoFornecedor: false`; só se afirma o que ele afirma.
      setAvisos(servico && !r.revogadoNoFornecedor ? t('mercado.desc.feito', { nome: c.name, servico }) : t('mercado.desc.feitoMcp', { nome: c.name }))
    } catch (e) { setErro(frasesDaRecusa(t, e)) }
  }

  return (
    <div className="mercado">
      {mercado.recemAdicionado ? <Aviso icon="checkmark.circle">{t('personalizado.minhas.adicionado', { nome: mercado.recemAdicionado })}</Aviso> : null}
      {avisos ? <Aviso alerta>{avisos}</Aviso> : null}
      {erro ? <Erro>{erro}</Erro> : null}
      {agruparContas(contas, catalogo).map((g) => (
        <section key={g.servico ?? 'proprios'} className="mercado-grupo" aria-label={nomeDoGrupo(g)}>
          <div className="mercado-grupo-nome">
            <LogoServico nome={nomeDoGrupo(g)} marca={g.marca} size={26} />
            <Text as="h3" size="headline" weight="semibold" tone="text">{nomeDoGrupo(g)}</Text>
            <Text size="callout" tone="secondary">{t.plural('mercado.contas.n', g.contas.length)}</Text>
            {(() => {
              const s = servicoParaAdicionar(g)
              return s ? <span className="mercado-grupo-adicionar"><Button kind="secondary" onClick={() => aoAdicionar(s)}><Icon name="plus" size={12} />{t('mercado.ficha.conectarOutra')}</Button></span> : null
            })()}
          </div>
          <div>
            {g.contas.map((c) => (
              <LinhaDaConta key={c.id} jeito={jeitoDaConta(c, catalogo)} conta={c} editando={renomeando === c.id} reconectando={reconectando === c.id}
                aoRenomear={() => { setRenomeando(c.id); setAvisos(null) }} aoFecharEdicao={() => setRenomeando(null)}
                salvarNome={(nome) => mercado.renomear(c.id, nome)} aoReconectar={() => void reconectar(c)} aoDesconectar={() => { setAvisos(null); setDesconectando(c) }} />
            ))}
          </div>
        </section>
      ))}
      <HStack gap={8}><Icon name="info.circle" size={13} /><Text size="callout" tone="secondary">{t('mercado.minhas.rodape')}</Text></HStack>
      <DialogoDesconectar conta={desconectando} servico={desconectando ? nomeDoServico(desconectando) : null} idioma={idioma}
        aoFechar={() => setDesconectando(null)} aoConfirmar={(c) => void desconectar(c)} />
    </div>
  )
}

function LinhaDaConta({ jeito, conta, editando, reconectando, aoRenomear, aoFecharEdicao, salvarNome, aoReconectar, aoDesconectar }: {
  jeito: TipoId | null; conta: Conta; editando: boolean; reconectando: boolean
  aoRenomear: () => void; aoFecharEdicao: () => void; salvarNome: (nome: string) => Promise<void>
  aoReconectar: () => void; aoDesconectar: () => void
}) {
  const t = useT()
  const idioma = useIdioma()
  const bots = botsQueUsam(conta)
  const expirada = contaExpirada(conta)
  const tipo = conta.servico === null ? t('mercado.minhas.mcp') : jeito === 'oficial_chave' ? t('mercado.minhas.chave') : t('mercado.minhas.entrou')
  return (
    <div className="mercado-conta">
      {editando ? (
        <EdicaoDoNome inicial={conta.name} salvar={salvarNome} aoFechar={aoFecharEdicao} />
      ) : (
        <>
          <div className="mercado-conta-nome">
            <HStack gap={8} shrink>
              <Text size="body" weight="semibold" tone="text" truncate>{conta.name}</Text>
              {conta.naoVerificado ? <HStack gap={4} className="mercado-etiqueta aviso"><Icon name="shield" size={10} /><Text size="caption" weight="semibold">{t('personalizado.minhas.selo')}</Text></HStack> : null}
            </HStack>
            {conta.identidade ? <Text size="callout" tone="secondary" truncate>{conta.identidade}</Text> : null}
          </div>
          <Text size="callout" tone="secondary">{tipo}</Text>
        </>
      )}
      <Text size="callout" tone={bots.length ? 'text' : 'secondary'} truncate>{bots.length ? bots.map((b) => b.name).join(', ') : t('mercado.minhas.semBot')}</Text>
      <HStack gap={6}>
        <PontoDeEstado alerta={expirada} />
        <Text size="callout" tone={expirada ? 'attention' : 'text'}>{expirada ? t('mercado.minhas.expirou') : t('mercado.minhas.ok')}</Text>
      </HStack>
      <div className="mercado-conta-fim">
        {expirada ? <Button onClick={aoReconectar} disabled={reconectando}><Icon name="arrow.clockwise" size={12} />{reconectando ? t('mercado.minhas.reconectando') : t('mercado.minhas.reconectar')}</Button> : null}
        <DropdownMenu language={superficieIdioma(idioma)} label={t('mercado.minhas.menu', { nome: conta.name })} title={t('mercado.minhas.menu', { nome: conta.name })} className="icon-button"
          items={() => [
            { title: t('mercado.minhas.renomear'), icon: 'pencil', action: aoRenomear },
            { title: t('mercado.minhas.desconectar'), icon: 'trash', destructive: true, action: aoDesconectar },
          ]}>
          <Icon name="ellipsis" size={14} />
        </DropdownMenu>
      </div>
    </div>
  )
}

/** O nome editado na própria linha: Enter salva, Esc cancela; a recusa do Hub (nome repetido…) aparece aqui, sem fechar. */
function EdicaoDoNome({ inicial, salvar, aoFechar }: { inicial: string; salvar: (nome: string) => Promise<void>; aoFechar: () => void }) {
  const t = useT()
  const [nome, setNome] = useState(inicial)
  const [erro, setErro] = useState<string | null>(null)
  const [ocupado, setOcupado] = useState(false)
  const campo = useRef<HTMLInputElement>(null)
  useEffect(() => { campo.current?.focus(); campo.current?.select() }, [])
  const gravar = async () => {
    if (ocupado || nome.trim() === '') return
    if (nome.trim() === inicial) { aoFechar(); return }
    setOcupado(true); setErro(null)
    try { await salvar(nome.trim()); aoFechar() } catch (e) { setErro(frasesDaRecusa(t, e, nome)); setOcupado(false) }
  }
  return (
    <VStack gap={4} className="mercado-edicao">
      <div className="mercado-renomear">
        <input ref={campo} className="mercado-campo" aria-label={t('mercado.minhas.nomeCampo')} aria-invalid={erro !== null} value={nome}
          onChange={(e) => { setNome(e.target.value); setErro(null) }}
          onKeyDown={(e) => { if (e.key === 'Enter') { e.preventDefault(); void gravar() } else if (e.key === 'Escape') { e.stopPropagation(); aoFechar() } }} />
        <Button onClick={() => void gravar()} disabled={ocupado || nome.trim() === ''}>{t('mercado.minhas.salvar')}</Button>
        <button className="mercado-botao-texto" onClick={aoFechar}><Text size="body" weight="medium">{t('mercado.minhas.cancelar')}</Text></button>
      </div>
      {erro ? <Erro>{erro}</Erro> : null}
    </VStack>
  )
}

function DialogoDesconectar({ conta, servico, idioma, aoFechar, aoConfirmar }: {
  conta: Conta | null; servico: string | null; idioma: ReturnType<typeof useIdioma>; aoFechar: () => void; aoConfirmar: (c: Conta) => void
}) {
  const t = useT()
  const bots = conta ? botsQueUsam(conta) : []
  return (
    <Dialog open={conta !== null} title={t('mercado.desc.titulo', { nome: conta?.name ?? '' })} cancel={t('mercado.desc.cancelar')} onClose={aoFechar} language={superficieIdioma(idioma)}
      actions={conta ? [{ title: t('mercado.desc.confirmar'), destructive: true, action: () => aoConfirmar(conta) }] : []}
      message={conta ? (
        <VStack gap={10}>
          <Text size="body" tone="secondary">{bots.length ? t('mercado.desc.bots') : t('mercado.desc.nenhumBot')}</Text>
          {bots.length ? <div className="mercado-chips">{bots.map((b) => <span key={b.id} className="mercado-chip"><Text size="callout" tone="text">{b.name}</Text></span>)}</div> : null}
          <Text size="body" tone="secondary">{servico ? t('mercado.desc.honesto', { servico }) : t('mercado.desc.honestoMcp')}</Text>
          <Text size="callout" tone="secondary">{t('mercado.desc.registro')}</Text>
        </VStack>
      ) : null} />
  )
}
