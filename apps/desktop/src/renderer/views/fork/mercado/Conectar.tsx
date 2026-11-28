// Telas 03, 03b e 04: conectar uma conta pela ficha. Configurar (nomes, liberações, ações perigosas à parte), entrar,
// conferir e "Usar em quais bots?". O fluxo (fluxo.ts) decide; aqui só se desenha cada passo e se enviam intenções.
import { useEffect, useRef, useState } from 'react'
import { Button, Spinner } from '../../../components/Controls'
import { Icon } from '../../../components/Icon'
import { HStack, VStack } from '../../../components/Stack'
import { Text } from '../../../components/Text'
import { useModel } from '../../../lib/observable'
import { useStore } from '../../../store/context'
import { useT } from '../../../lib/fork-i18n/estado'
import type { FluxoConectar, Passo } from './fluxo.ts'
import { EscolherBots } from './EscolherBots'
import { botsParaLigar, escolhasDeBots, gruposEscondidos, gruposVisiveis, mostraPersonalizar, padraoSeguro, perigosasDoTipo } from './regras.ts'
import type { Conta } from './tipos.ts'
import { SkillsSugeridas } from './SkillsSugeridas'
import { Aviso, Erro, frasesDaRecusa, LogoServico, Titulo } from './pecas'


const NUMERO: Record<Passo, 1 | 2 | 3> = { configurar: 1, entrando: 2, chave: 2, conferindo: 2, pronto: 3 }

export function Conectar({ fluxo, contaViva, usarNosBots, aoVoltar, aoConcluir }: {
  fluxo: FluxoConectar
  /** A conta como o Hub a lista agora (com os bots do Space), para o passo "Usar em quais bots?". */
  contaViva: Conta | null
  usarNosBots: (contaId: string, botIds: string[]) => Promise<void>
  aoVoltar: () => void
  aoConcluir: () => void
}) {
  useModel(fluxo)
  const t = useT()
  const cancelar = () => { void fluxo.cancelar().finally(aoVoltar) }
  return (
    <div className="mercado">
      <div className="mercado-topo">
        <button className="mercado-voltar" onClick={fluxo.passo === 'pronto' ? aoConcluir : cancelar}>
          <Icon name="arrow.left" size={12} /><Text size="callout">{fluxo.passo === 'pronto' ? t('mercado.ficha.voltar') : t('mercado.conectar.voltar', { servico: fluxo.servico.nome })}</Text>
        </button>
        <Passos passo={fluxo.passo} />
      </div>
      {fluxo.passo === 'configurar' ? <Configurar fluxo={fluxo} aoCancelar={cancelar} /> : null}
      {fluxo.passo === 'entrando' ? <Entrando fluxo={fluxo} aoCancelar={cancelar} /> : null}
      {fluxo.passo === 'chave' ? <Chave fluxo={fluxo} aoCancelar={cancelar} /> : null}
      {fluxo.passo === 'conferindo' ? <Conferindo fluxo={fluxo} /> : null}
      {fluxo.passo === 'pronto' ? <Pronto fluxo={fluxo} conta={contaViva} usarNosBots={usarNosBots} aoConcluir={aoConcluir} /> : null}
    </div>
  )
}

function Passos({ passo }: { passo: Passo }) {
  const t = useT()
  const n = NUMERO[passo]
  const itens = [
    { i: 1, chave: 'mercado.conectar.passo.configurar' },
    { i: 2, chave: 'mercado.conectar.passo.entrar' },
    { i: 3, chave: 'mercado.conectar.passo.skills' },
  ] as const
  return (
    <ol className="mercado-passos" aria-label={t('mercado.conectar.passos')}>
      {itens.map((p) => (
        <li key={p.i} className={`mercado-passo${p.i < n ? ' feito' : ''}`} aria-current={p.i === n ? 'step' : undefined}>
          <span className="mercado-passo-numero"><Text size="caption2" weight="semibold">{p.i < n ? '✓' : p.i}</Text></span>
          <Text size="callout" weight={p.i === n ? 'semibold' : 'regular'}>{t(p.chave)}</Text>
        </li>
      ))}
    </ol>
  )
}

function Cabecalho({ fluxo }: { fluxo: FluxoConectar }) {
  const t = useT()
  const outra = (fluxo.servico.contas?.length ?? 0) > 0
  return (
    <div className="mercado-cabeca">
      <LogoServico nome={fluxo.servico.nome} marca={fluxo.servico.marca} size={48} />
      <VStack gap={2} shrink>
        <Text as="h2" size="title2" weight="semibold" tone="text">{t(outra ? 'mercado.conectar.tituloOutra' : 'mercado.conectar.titulo', { servico: fluxo.servico.nome })}</Text>
        <Text size="callout" tone="secondary">{t(fluxo.tipo.auth.modo === 'chave' ? 'mercado.conectar.subChave' : 'mercado.conectar.subOauth', { servico: fluxo.servico.nome })}</Text>
      </VStack>
    </div>
  )
}

function CartaoAppProprio({ fluxo, cadastro }: { fluxo: FluxoConectar; cadastro: string }) {
  const t = useT()
  const [editar, setEditar] = useState(false)
  const [id, setId] = useState('')
  const [chave, setChave] = useState('')
  const salvo = fluxo.appProprio?.configurado === true
  const escrever = (remover: boolean) => {
    const dados = remover ? undefined : { id, chave }
    setId(''); setChave('')
    void fluxo.escreverAppProprio(dados).then((ok) => { if (ok) setEditar(false) })
  }
  return (
    <section className="mercado-secao">
      <Titulo titulo={t('mercado.appProprio.titulo')} sub={t('google.cartao.texto')} />
      {fluxo.appProprio === null ? <Aviso>{t('mercado.appProprio.indisponivel')}</Aviso> :
        salvo && !editar ? (
          <Button kind="secondary" onClick={() => setEditar(true)} disabled={fluxo.ocupado}>{t('mercado.appProprio.editar')}</Button>
        ) : (
          <div className="mercado-bloco">
            <div>
              <Button kind="secondary" onClick={() => {
                if (URL.canParse(cadastro) && new URL(cadastro).protocol === 'https:') window.codync.app.openExternal(cadastro)
              }}>{t('mercado.appProprio.portal')}</Button>
            </div>
            <Text size="callout" tone="secondary">{t('mercado.appProprio.redirect')}</Text>
            {fluxo.cadastro?.redirects.map((url) => <Text key={url} as="div" size="caption" mono className="mercado-mono" dir="ltr">{url}</Text>)}
            {salvo ? <Aviso alerta>{t('mercado.appProprio.aviso')}</Aviso> : null}
            <label>
              <Text as="div" size="caption" weight="semibold">{t('google.cartao.id')}</Text>
              <input className="mercado-campo" value={id} autoComplete="off" spellCheck={false} disabled={fluxo.ocupado} onChange={(e) => setId(e.target.value)} />
            </label>
            <label>
              <Text as="div" size="caption" weight="semibold">{t('google.cartao.chave')}</Text>
              <input className="mercado-campo" type="password" value={chave} autoComplete="off" spellCheck={false} disabled={fluxo.ocupado} onChange={(e) => setChave(e.target.value)} />
            </label>
            <div className="mercado-acoes">
              <Button onClick={() => escrever(false)} disabled={fluxo.ocupado || !id.trim() || !chave.trim()}>{t('mercado.appProprio.salvar')}</Button>
              {salvo ? <Button kind="secondary" onClick={() => escrever(true)} disabled={fluxo.ocupado}>{t('mercado.appProprio.remover')}</Button> : null}
            </div>
          </div>
        )}
      {fluxo.erroAppProprio ? <Erro>{t('mercado.appProprio.erro')}</Erro> : null}
    </section>
  )
}

function Configurar({ fluxo, aoCancelar }: { fluxo: FluxoConectar; aoCancelar: () => void }) {
  const t = useT()
  const [ferramentas, setFerramentas] = useState(false)
  const nomes = fluxo.nomes
  const novoRef = useRef<HTMLInputElement>(null)
  useEffect(() => { novoRef.current?.focus({ preventScroll: true }) }, [])
  const visiveis = gruposVisiveis(fluxo.tipo, fluxo.personalizando)
  const escondidos = gruposEscondidos(fluxo.tipo)
  const perigosas = perigosasDoTipo(fluxo.tipo)
  const noPadrao = fluxo.escolhidos.length === padraoSeguro(fluxo.tipo).length && padraoSeguro(fluxo.tipo).every((id) => fluxo.escolhidos.includes(id))
  const erro = fluxo.erro
  const erroNome = (campo: 'nova' | 'primeira') => erro && erro.campo === campo ? frasesDaRecusa(t, erro.texto, fluxo.nomeDaNova) : null
  const erroGeral = erro && erro.campo === null ? frasesDaRecusa(t, erro.texto) : null
  const cadastro = fluxo.tipo.auth.modo === 'app_proprio' ? fluxo.tipo.auth.configuracao?.cadastro : undefined
  const continuar = fluxo.tipo.auth.modo !== 'chave' ? t('mercado.rodape.continuarSite', { servico: fluxo.servico.nome }) : t('mercado.rodape.continuar')
  return (
    <>
      <Cabecalho fluxo={fluxo} />
      <div className="mercado-conectar">
        <div className="mercado-ficha-principal">
          {cadastro ? <CartaoAppProprio fluxo={fluxo} cadastro={cadastro} /> : null}
          <section className="mercado-secao">
            <Titulo titulo={t(nomes.pedeNomeDaNova ? 'mercado.nomes.titulo.dois' : 'mercado.nomes.titulo.um')} sub={nomes.pedeNomeDaNova ? t('mercado.nomes.sub', { servico: fluxo.servico.nome }) : undefined} />
            {nomes.pedeNomeDaNova ? (
              <div className="mercado-bloco">
                {nomes.pedeNomeDaPrimeira && nomes.primeira ? (
                  <div className="mercado-campo-linha">
                    <VStack gap={2}>
                      <Text size="callout" weight="semibold" tone="text">{t('mercado.nomes.primeira.titulo')}</Text>
                      <Text size="caption" tone="secondary">{t('mercado.nomes.primeira.nota', { nome: nomes.primeira.name })}</Text>
                      {nomes.primeira.identidade ? <Text size="caption" tone="secondary">{nomes.primeira.identidade}</Text> : null}
                    </VStack>
                    <VStack gap={4}>
                      <input className="mercado-campo" aria-label={t('mercado.nomes.campo.primeira')} aria-invalid={erroNome('primeira') !== null} value={fluxo.nomeDaPrimeira} placeholder={t('mercado.nomes.placeholder', { servico: fluxo.servico.nome })} onChange={(e) => fluxo.nomear('primeira', e.target.value)} />
                      {erroNome('primeira') ? <Erro>{erroNome('primeira')}</Erro> : null}
                    </VStack>
                  </div>
                ) : null}
                <div className="mercado-campo-linha">
                  <VStack gap={2}>
                    <Text size="callout" weight="semibold" tone="text">{t('mercado.nomes.nova.titulo')}</Text>
                    <Text size="caption" tone="secondary">{t('mercado.nomes.obrigatorio')}</Text>
                  </VStack>
                  <VStack gap={4}>
                    <input ref={novoRef} className="mercado-campo" aria-label={t('mercado.nomes.campo.nova')} aria-invalid={erroNome('nova') !== null} value={fluxo.nomeDaNova} placeholder={t('mercado.nomes.placeholder', { servico: fluxo.servico.nome })} onChange={(e) => fluxo.nomear('nova', e.target.value)} />
                    {erroNome('nova') ? <Erro>{erroNome('nova')}</Erro> : null}
                    {erro?.codigo === 'nome_repetido' ? <Text size="caption" tone="secondary">{t('mercado.nomes.dica')}</Text> : null}
                  </VStack>
                </div>
              </div>
            ) : (
              <div className="mercado-bloco">
                <HStack gap={12}>
                  <LogoServico nome={fluxo.servico.nome} marca={fluxo.servico.marca} size={32} />
                  <VStack gap={2} shrink>
                    <Text size="body" weight="semibold" tone="text">{fluxo.servico.nome}</Text>
                    <Text size="callout" tone="secondary">{t('mercado.nomes.primeiraConta', { servico: fluxo.servico.nome })}</Text>
                  </VStack>
                </HStack>
              </div>
            )}
          </section>

          <section className="mercado-secao">
            <div className="mercado-secao-topo">
              <Titulo titulo={t('mercado.lib.titulo')} sub={t('mercado.lib.sub')} />
              {!noPadrao ? <button className="mercado-link" onClick={() => fluxo.voltarAoPadrao()}><Text size="callout" weight="semibold">{t('mercado.lib.voltar')}</Text></button> : null}
            </div>
            <div className="mercado-liberacoes" role="group" aria-label={t('mercado.lib.titulo')}>
              {visiveis.map((g) => (
                <label key={g.id} className="mercado-liberacao" aria-disabled={!g.disponivel}>
                  <input type="checkbox" checked={fluxo.escolhidos.includes(g.id)} disabled={!g.disponivel} onChange={() => fluxo.alternar(g.id)} />
                  <VStack gap={2} shrink>
                    <HStack gap={8}>
                      <Text size="body" weight="medium" tone="text">{g.rotulo}</Text>
                      {g.padrao && g.disponivel && g.imposicao !== 'filtro' ? <Text size="caption" weight="medium" className="mercado-etiqueta neutra">{t('mercado.lib.padrao')}</Text> : null}
                      {!g.disponivel ? <Text size="caption" tone="tertiary">{t('mercado.lib.indisponivel')}</Text> : null}
                    </HStack>
                  </VStack>
                </label>
              ))}
              {mostraPersonalizar(fluxo.tipo) && !fluxo.personalizando ? (
                <button className="mercado-personalizar" onClick={() => fluxo.personalizar()}>
                  <Icon name="plus" size={12} />
                  <VStack gap={0} grow shrink>
                    <Text size="body" weight="medium" tone="text">{t('mercado.lib.personalizar')}</Text>
                    <Text size="callout" tone="secondary">{t('mercado.lib.mais', { n: escondidos.length, lista: escondidos.map((g) => g.rotulo).join(', ') })}</Text>
                  </VStack>
                  <Icon name="chevron.right" size={11} />
                </button>
              ) : null}
            </div>
            <button className="mercado-link discreto" aria-expanded={ferramentas} onClick={() => setFerramentas(!ferramentas)}>
              <HStack gap={4}><Text size="callout">{t(ferramentas ? 'mercado.lib.ferramentas.ocultar' : 'mercado.lib.ferramentas.ver')}</Text><Icon name={ferramentas ? 'chevron.up' : 'chevron.down'} size={10} /></HStack>
            </button>
            {ferramentas ? (
              <div className="mercado-detalhes">
                {fluxo.tipo.grupos.map((g) => (
                  <VStack key={g.id} gap={2}>
                    <Text size="callout" weight="semibold" tone="text">{g.rotulo}</Text>
                    {g.ferramentas.length > 0 ? <Text size="caption" mono className="mercado-mono">{g.ferramentas.join(', ')}</Text> : <Text size="caption" tone="secondary">{t(g.imposicao === 'filtro' ? 'mercado.lib.filtro' : 'mercado.lib.ferramentas.fornecedor')}</Text>}
                  </VStack>
                ))}
              </div>
            ) : null}
            {fluxo.credencialComEscrita ? <Aviso alerta icon="exclamationmark.triangle">{t('mercado.lib.filtro')}</Aviso> : null}
            {fluxo.escolhidos.length === 0 ? <Erro>{t('mercado.lib.nenhuma')}</Erro> : null}
          </section>
          {erroGeral ? <Erro>{erroGeral}</Erro> : null}
        </div>

        {perigosas.length > 0 ? (
          <aside className="mercado-ficha-lateral">
            <div className="mercado-perigosas">
              <HStack gap={10} align="baseline">
                <Icon name="exclamationmark.triangle" size={14} />
                <VStack gap={2} shrink>
                  <Text size="body" weight="semibold" tone="text">{t('mercado.perigosas.titulo')}</Text>
                  <Text size="callout" tone="secondary">{t('mercado.perigosas.texto')}</Text>
                </VStack>
              </HStack>
              {perigosas.map((p) => (
                <div key={p.ferramenta} className="mercado-perigosa">
                  <Text size="body" tone="text">{p.rotulo}</Text>
                  <Text size="callout" tone="secondary">{t('mercado.perigosas.desligadas')}</Text>
                </div>
              ))}
            </div>
          </aside>
        ) : null}
      </div>
      <div className="mercado-rodape">
        <HStack gap={8} shrink><Icon name="info.circle" size={13} /><Text size="callout" tone="secondary">{t('mercado.rodape.space')}</Text></HStack>
        <div className="mercado-acoes">
          <button className="mercado-botao-texto" onClick={aoCancelar}><Text size="body" weight="medium">{t('mercado.rodape.cancelar')}</Text></button>
          <Button onClick={() => void fluxo.confirmar()} disabled={!fluxo.podeContinuar}>{continuar}</Button>
        </div>
      </div>
    </>
  )
}

function Entrando({ fluxo, aoCancelar }: { fluxo: FluxoConectar; aoCancelar: () => void }) {
  const t = useT()
  const falhou = fluxo.falhaDeEntrada !== null
  const [detalhes, setDetalhes] = useState(false)
  return (
    <>
      <Cabecalho fluxo={fluxo} />
      <div className="mercado-estado" role="status" aria-live="polite">
        <div className="mercado-estado-topo">
          {falhou ? <Icon name="xmark.circle" size={22} /> : <Spinner size={20} />}
          <VStack gap={4} shrink>
            <Text as="h3" size="headline" weight="semibold" tone="text">{t(falhou ? 'mercado.entrando.falhou.titulo' : 'mercado.entrando.titulo', { servico: fluxo.servico.nome })}</Text>
            <Text size="callout" tone="secondary">{t(falhou ? 'mercado.entrando.falhou.texto' : 'mercado.entrando.texto', { servico: fluxo.servico.nome })}</Text>
          </VStack>
        </div>
        <div className="mercado-acoes">
          {falhou ? <Button onClick={() => void fluxo.entrar()}>{t('mercado.entrando.tentar')}</Button> : null}
          <button className="mercado-botao-texto" onClick={aoCancelar}><Text size="body" weight="medium">{t('mercado.entrando.cancelar')}</Text></button>
          {falhou ? <button className="mercado-link discreto" aria-expanded={detalhes} onClick={() => setDetalhes(!detalhes)}><Text size="callout">{t('mercado.erro.detalhes')}</Text></button> : null}
        </div>
        {falhou && detalhes ? <div className="mercado-detalhes"><Text size="caption" mono className="mercado-mono">{fluxo.falhaDeEntrada}</Text></div> : null}
      </div>
    </>
  )
}

function Chave({ fluxo, aoCancelar }: { fluxo: FluxoConectar; aoCancelar: () => void }) {
  const t = useT()
  const [valor, setValor] = useState('')
  const [detalhes, setDetalhes] = useState(false)
  const ajuda = fluxo.tipo.auth.modo === 'chave' ? fluxo.tipo.auth.comoCriarLeitura : ''
  const falhou = fluxo.falhaDeEntrada !== null
  return (
    <>
      <Cabecalho fluxo={fluxo} />
      <div className="mercado-estado">
        {falhou ? (
          <div className="mercado-estado-topo">
            <Icon name="key" size={20} />
            <VStack gap={4} shrink>
              <Text as="h3" size="headline" weight="semibold" tone="text">{t('mercado.chave.recusada.titulo', { servico: fluxo.servico.nome })}</Text>
              <Text size="callout" tone="secondary">{t('mercado.chave.recusada.texto')}</Text>
            </VStack>
          </div>
        ) : <Titulo titulo={t('mercado.chave.titulo')} sub={ajuda || undefined} />}
        <input type="password" autoComplete="off" spellCheck={false} className="mercado-campo" aria-label={t('mercado.chave.campo')} aria-invalid={falhou} value={valor} onChange={(e) => setValor(e.target.value)} />
        <Text size="caption" tone="secondary">{t('mercado.chave.nota')}</Text>
        <div className="mercado-acoes">
          <Button onClick={() => void fluxo.enviarChave(valor).then(() => setValor(''))} disabled={fluxo.ocupado || valor.trim() === ''}>{t('mercado.chave.guardar')}</Button>
          <button className="mercado-botao-texto" onClick={aoCancelar}><Text size="body" weight="medium">{t('mercado.rodape.cancelar')}</Text></button>
          {falhou ? <button className="mercado-link discreto" aria-expanded={detalhes} onClick={() => setDetalhes(!detalhes)}><Text size="callout">{t('mercado.erro.detalhes')}</Text></button> : null}
        </div>
        {falhou && detalhes ? <div className="mercado-detalhes"><Text size="caption" mono className="mercado-mono">{fluxo.falhaDeEntrada}</Text></div> : null}
      </div>
    </>
  )
}

function Conferindo({ fluxo }: { fluxo: FluxoConectar }) {
  const t = useT()
  const nome = fluxo.conta?.name ?? fluxo.servico.nome
  return (
    <div className="mercado-estado" role="status" aria-live="polite">
      <HStack gap={12}>
        <LogoServico nome={fluxo.servico.nome} marca={fluxo.servico.marca} size={44} />
        <VStack gap={2} shrink>
          <Text as="h3" size="headline" weight="semibold" tone="text">{t('mercado.conferindo.titulo', { conta: nome })}</Text>
          <Text size="callout" tone="secondary">{t('mercado.conferindo.texto')}</Text>
        </VStack>
      </HStack>
      <div className="mercado-etapas">
        <div className="mercado-etapa feita"><Icon name="checkmark.circle.fill" size={16} /><Text size="body">{t('mercado.conferindo.entrou', { servico: fluxo.servico.nome })}</Text></div>
        <div className="mercado-etapa agora"><Spinner size={14} /><Text size="body">{t('mercado.conferindo.vendo')}</Text></div>
        <div className="mercado-etapa"><Icon name="circle" size={16} /><Text size="body">{t('mercado.conferindo.proximo')}</Text></div>
      </div>
    </div>
  )
}

function Pronto({ fluxo, conta, usarNosBots, aoConcluir }: { fluxo: FluxoConectar; conta: Conta | null; usarNosBots: (id: string, ids: string[]) => Promise<void>; aoConcluir: () => void }) {
  const t = useT()
  const [marcados, setMarcados] = useState<Set<string>>(new Set())
  const [salvando, setSalvando] = useState(false)
  const [falha, setFalha] = useState<string | null>(null)
  const store = useStore()
  const principal = useModel(store.principal)
  const times = useModel(store.timesRoster)
  const bots = conta?.bots ?? []
  // O principal vem do mesmo lugar da tabela "Contas no bot" (`principal.protectedId`), nunca do nome.
  const escolhas = escolhasDeBots(bots, (id) => store.bots.get(id)?.title, times.snapshot, principal.protectedId)
  const nome = conta?.name ?? fluxo.conta?.name ?? fluxo.servico.nome
  const concluir = async () => {
    // O principal nunca sai daqui (o Hub o recusa de qualquer jeito): botsParaLigar o tira da lista.
    const ids = botsParaLigar(marcados, escolhas)
    if (ids.length === 0 || !conta) { aoConcluir(); return }
    setSalvando(true); setFalha(null)
    try { await usarNosBots(conta.id, ids); aoConcluir() } catch (e) { setFalha(frasesDaRecusa(t, e)); setSalvando(false) }
  }
  return (
    <>
      <div className="mercado-conectar">
        <div className="mercado-ficha-principal">
          <div className="mercado-pronta" role="status">
            <Icon name="checkmark.circle.fill" size={30} color="var(--success)" />
            <VStack gap={2} shrink>
              <Text as="h2" size="title3" weight="semibold" tone="text">{t('mercado.pronto.titulo', { conta: nome })}</Text>
              <Text size="callout" tone="secondary">{conta?.identidade ? t('mercado.pronto.sub', { identidade: conta.identidade }) : t('mercado.pronto.subSem')}</Text>
            </VStack>
          </div>
          <SkillsSugeridas servico={fluxo.servico} />
        </div>
        <aside className="mercado-ficha-lateral">
          <Titulo titulo={t('mercado.bots.titulo')} sub={t('mercado.bots.sub')} />
          {bots.length === 0 ? <Aviso>{t('mercado.bots.nenhum')}</Aviso> : <EscolherBots escolhas={escolhas} marcados={marcados} aoMudar={setMarcados} desabilitado={salvando} />}
          {escolhas.some((b) => b.principal) ? null : <Aviso>{t('mercado.bots.berga')}</Aviso>}
          {falha ? <Erro>{falha}</Erro> : null}
        </aside>
      </div>
      <div className="mercado-rodape">
        <HStack gap={8} shrink><Icon name="info.circle" size={13} /><Text size="callout" tone="secondary">{t('mercado.pronto.rodape')}</Text></HStack>
        <Button onClick={() => void concluir()} disabled={salvando}>{salvando ? t('mercado.pronto.salvando') : t('mercado.pronto.concluir')}</Button>
      </div>
    </>
  )
}
