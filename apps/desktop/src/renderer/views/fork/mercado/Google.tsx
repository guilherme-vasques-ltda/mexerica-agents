// Telas 03 e 04 do Google direto: o guia de 6 passos com o cartão seguro (ID e chave secreta), a escolha dos serviços com
// as liberações e o login em fila. O fluxo (google-fluxo.ts) decide; aqui só se desenha cada passo e se enviam intenções.
import { useState } from 'react'
import { Button, Spinner } from '../../../components/Controls'
import { Icon } from '../../../components/Icon'
import { HStack, VStack } from '../../../components/Stack'
import { Text } from '../../../components/Text'
import { useT } from '../../../lib/fork-i18n/estado'
import type { ChaveTexto } from '../../../lib/fork-i18n/index.ts'
import { useModel } from '../../../lib/observable'
import type { FluxoGoogle, PassoGoogle } from './google-fluxo.ts'
import { guiaDoGoogle, padraoSeguro } from './regras.ts'
import type { Servico } from './tipos.ts'
import { Aviso, Erro, frasesDaRecusa, LogoServico, Titulo } from './pecas'

const NUMERO: Record<PassoGoogle, 1 | 2 | 3> = { guia: 1, servicos: 2, entrar: 3, pronto: 3 }
/** Os passos do guia que a tela sabe contar; um passo novo da ficha sem texto aqui some em vez de aparecer em branco. */
const PASSOS_COM_TEXTO = ['projeto', 'apis', 'tela_login', 'escopos', 'cliente'] as const

const tx = (t: unknown, chave: string): string => (t as (c: ChaveTexto) => string)(chave as ChaveTexto)

/** Só abre link https (os da ficha); qualquer outra coisa não faz nada. */
const abrirLink = (url: string) => { if (URL.canParse(url) && new URL(url).protocol === 'https:') window.codync.app.openExternal(url) }

export function Google({ fluxo, aoSair, aoConcluir }: { fluxo: FluxoGoogle; aoSair: () => void; aoConcluir: () => void }) {
  useModel(fluxo)
  const t = useT()
  const sair = () => { void fluxo.cancelar().finally(aoSair) }
  return (
    <div className="mercado">
      <div className="mercado-topo">
        <button className="mercado-voltar" onClick={fluxo.passo === 'pronto' ? aoConcluir : sair}>
          <Icon name="arrow.left" size={12} /><Text size="callout">{t('google.guia.voltar')}</Text>
        </button>
        <Passos passo={fluxo.passo} />
      </div>
      {fluxo.passo === 'guia' ? <Guia fluxo={fluxo} aoCancelar={sair} /> : null}
      {fluxo.passo === 'servicos' ? <Servicos fluxo={fluxo} aoCancelar={sair} /> : null}
      {fluxo.passo === 'entrar' || fluxo.passo === 'pronto' ? <Entrar fluxo={fluxo} aoCancelar={sair} aoConcluir={aoConcluir} /> : null}
    </div>
  )
}

function Passos({ passo }: { passo: PassoGoogle }) {
  const t = useT()
  const n = NUMERO[passo]
  const itens = [
    { i: 1, chave: 'google.guia.etapa.app' },
    { i: 2, chave: 'google.guia.etapa.servicos' },
    { i: 3, chave: 'google.guia.etapa.entrar' },
  ] as const
  return (
    <ol className="mercado-passos" aria-label={t('mercado.conectar.passos')}>
      {itens.map((p) => (
        <li key={p.i} className={`mercado-passo${p.i < n || (passo === 'pronto' && p.i === n) ? ' feito' : ''}`} aria-current={p.i === n && passo !== 'pronto' ? 'step' : undefined}>
          <span className="mercado-passo-numero"><Text size="caption2" weight="semibold">{p.i < n || (passo === 'pronto' && p.i === n) ? '✓' : p.i}</Text></span>
          <Text size="callout" weight={p.i === n ? 'semibold' : 'regular'}>{t(p.chave)}</Text>
        </li>
      ))}
    </ol>
  )
}

function Logos({ servicos, size = 40 }: { servicos: readonly Servico[]; size?: number }) {
  return <div className="mercado-google-logos" aria-hidden>{servicos.map((s) => <LogoServico key={s.id} nome={s.nome} marca={s.marca} size={size} />)}</div>
}

// ───────────────────────────── Tela 03: o guia e o cartão seguro ─────────────────────────────

function Guia({ fluxo, aoCancelar }: { fluxo: FluxoGoogle; aoCancelar: () => void }) {
  const t = useT()
  // Os dois valores moram só neste estado, até o envio ao Hub; nunca vão para o modelo, o log ou o catálogo.
  const [id, setId] = useState('')
  const [chave, setChave] = useState('')
  const [mostrar, setMostrar] = useState(false)
  const passos = guiaDoGoogle(fluxo.servicos).filter((p) => (PASSOS_COM_TEXTO as readonly string[]).includes(p.id))
  const erro = fluxo.erroDoApp
  const salvar = () => { void fluxo.salvarApp(id, chave).then((ok) => { if (ok) { setId(''); setChave('') } }) }
  return (
    <>
      <div className="mercado-conectar">
        <div className="mercado-ficha-principal">
          <div className="mercado-cabeca">
            <Logos servicos={fluxo.servicos} />
            <VStack gap={2} shrink>
              <Text as="h2" size="title2" weight="semibold" tone="text">{t('google.guia.titulo')}</Text>
              <Text size="callout" tone="secondary">{t('google.guia.sub')}</Text>
            </VStack>
          </div>
          {fluxo.configurado ? <Aviso icon="exclamationmark.triangle" alerta>{t('google.guia.trocarAviso')}</Aviso> : null}
          <ol className="mercado-guia">
            {passos.map((p, i) => (
              <li key={p.id} className="mercado-guia-passo">
                <span className="mercado-passo-numero"><Text size="caption" weight="semibold">{i + 1}</Text></span>
                <VStack gap={2} shrink grow>
                  <Text size="body" weight="semibold" tone="text">{tx(t, `google.passo.${p.id}.titulo`)}</Text>
                  <Text size="callout" tone="secondary">{tx(t, `google.passo.${p.id}.texto`)}</Text>
                </VStack>
                {p.url ? <Button kind="secondary" onClick={() => abrirLink(p.url!)}>{tx(t, `google.passo.${p.id}.botao`)}<Icon name="arrow.up.right.square" size={12} /></Button> : null}
              </li>
            ))}
          </ol>
        </div>

        <aside className="mercado-ficha-lateral">
          <div className="mercado-cartao" role="group" aria-label={t('google.passo.campos.titulo')}>
            <HStack gap={10} align="baseline">
              <span className="mercado-passo-numero destaque"><Text size="caption" weight="semibold">{passos.length + 1}</Text></span>
              <Text size="headline" weight="semibold" tone="text">{t('google.passo.campos.titulo')}</Text>
            </HStack>
            <div className="mercado-cartao-miolo">
              <HStack gap={8}>
                <Icon name="lock" size={13} />
                <Text size="body" weight="semibold" tone="text">{t('google.cartao.titulo')}</Text>
                <Text size="caption" weight="medium" className="mercado-etiqueta neutra">{t('google.cartao.selo')}</Text>
              </HStack>
              <Text size="callout" tone="secondary">{t('google.cartao.texto')}</Text>
              <label className="mercado-cartao-campo">
                <HStack gap={6} align="baseline"><Text size="callout" weight="semibold" tone="text">{t('google.cartao.id')}</Text><Text size="caption" tone="secondary">{t('google.cartao.idDica')}</Text></HStack>
                <input className="mercado-campo mercado-mono" autoComplete="off" spellCheck={false} autoCapitalize="off" value={id} aria-invalid={erro !== null} onChange={(e) => setId(e.target.value)} placeholder={t('google.cartao.idExemplo')} />
              </label>
              <label className="mercado-cartao-campo">
                <HStack gap={6} align="baseline"><Text size="callout" weight="semibold" tone="text">{t('google.cartao.chave')}</Text><Text size="caption" tone="secondary">{t('google.cartao.chaveDica')}</Text></HStack>
                <input className="mercado-campo mercado-mono" type={mostrar ? 'text' : 'password'} autoComplete="off" spellCheck={false} autoCapitalize="off" value={chave} aria-invalid={erro !== null} onChange={(e) => setChave(e.target.value)} placeholder={t('google.cartao.chaveExemplo')} />
              </label>
              <button className="mercado-link discreto" onClick={() => setMostrar(!mostrar)}><Text size="callout">{t(mostrar ? 'google.cartao.esconder' : 'google.cartao.mostrar')}</Text></button>
              <Text size="caption" tone="secondary">{t('google.cartao.dica')}</Text>
              {erro ? <Erro>{frasesDaRecusa(t, erro)}</Erro> : null}
            </div>
          </div>
          <Aviso>{t('google.guia.nota.seu')}</Aviso>
          <Aviso icon="clock">{t('google.guia.nota.teste')}</Aviso>
        </aside>
      </div>
      <div className="mercado-rodape">
        <HStack gap={8} shrink><Icon name="info.circle" size={13} /><Text size="callout" tone="secondary">{t('google.guia.rodape')}</Text></HStack>
        <div className="mercado-acoes">
          <button className="mercado-botao-texto" onClick={fluxo.configurado ? () => fluxo.voltarAosServicos() : aoCancelar}><Text size="body" weight="medium">{t(fluxo.configurado ? 'google.servicos.voltar' : 'google.guia.cancelar')}</Text></button>
          <Button onClick={salvar} disabled={fluxo.ocupado || id.trim() === '' || chave.trim() === ''}>{t(fluxo.ocupado ? 'google.guia.salvando' : 'google.guia.salvar')}</Button>
        </div>
      </div>
    </>
  )
}

// ───────────────────────────── Tela 04: os serviços e as liberações ─────────────────────────────

function Servicos({ fluxo, aoCancelar }: { fluxo: FluxoGoogle; aoCancelar: () => void }) {
  const t = useT()
  const fim = (fluxo.app?.clientId ?? '').split('-')[1]?.split('.')[0] ?? ''
  const marcados = fluxo.servicos.filter((s) => fluxo.marcado(s.id))
  return (
    <>
      <div className="mercado-conectar">
        <div className="mercado-ficha-principal">
          <div className="mercado-pronta" role="status">
            <Icon name="checkmark.circle.fill" size={30} color="var(--success)" />
            <VStack gap={2} shrink grow>
              <Text as="h2" size="title3" weight="semibold" tone="text">{t('google.servicos.salvo')}</Text>
              <Text size="callout" tone="secondary">{t('google.servicos.salvoSub', { fim })}</Text>
            </VStack>
            <button className="mercado-link discreto" onClick={() => fluxo.trocarApp()}><Text size="callout">{t('google.servicos.trocar')}</Text></button>
          </div>
          <section className="mercado-secao">
            <div className="mercado-secao-topo">
              <Titulo titulo={t('google.servicos.titulo')} sub={t('google.servicos.sub')} />
              <Text size="callout" tone="secondary" tabular>{t('google.servicos.marcados', { n: fluxo.totalMarcados })}</Text>
            </div>
            <div className="mercado-liberacoes">
              {fluxo.servicos.map((s) => <ServicoLinha key={s.id} fluxo={fluxo} servico={s} />)}
            </div>
            {fluxo.totalMarcados === 0 ? <Erro>{t('google.servicos.nenhum')}</Erro> : null}
          </section>
        </div>
        <aside className="mercado-ficha-lateral">
          <Titulo titulo={t('google.contas.titulo')} sub={t('google.contas.sub')} />
          <div className="mercado-bloco">
            {marcados.map((s) => (
              <div key={s.id} className="mercado-linha">
                <LogoServico nome={s.nome} marca={s.marca} size={24} />
                <Text size="body" weight="medium" tone="text" truncate>{s.nome}</Text>
                <span className="mercado-card-fim"><PontoCinza /><Text size="caption" tone="secondary">{t('google.contas.desligada')}</Text></span>
              </div>
            ))}
          </div>
          <Aviso>{t('google.servicos.nota')}</Aviso>
        </aside>
      </div>
      <div className="mercado-rodape">
        <HStack gap={8} shrink><Icon name="info.circle" size={13} /><Text size="callout" tone="secondary">{t('google.servicos.rodape')}</Text></HStack>
        <div className="mercado-acoes">
          <button className="mercado-botao-texto" onClick={aoCancelar}><Text size="body" weight="medium">{t('google.guia.cancelar')}</Text></button>
          <Button onClick={() => void fluxo.continuar()} disabled={!fluxo.podeContinuar}>{t('google.servicos.continuar')}</Button>
        </div>
      </div>
    </>
  )
}

const PontoCinza = () => <span aria-hidden className="mercado-ponto neutro" />

function ServicoLinha({ fluxo, servico }: { fluxo: FluxoGoogle; servico: Servico }) {
  const t = useT()
  const f = fluxo.fluxo(servico.id)
  if (!f) return null
  const marcado = fluxo.marcado(servico.id)
  const nomes = f.nomes
  const erro = f.erro
  const erroNome = (campo: 'nova' | 'primeira') => (erro && erro.campo === campo ? frasesDaRecusa(t, erro.texto, f.nomeDaNova) : null)
  return (
    <div className="mercado-google-servico" aria-disabled={!marcado}>
      <label className="mercado-liberacao">
        <input type="checkbox" checked={marcado} onChange={() => fluxo.alternarServico(servico.id)} />
        <LogoServico nome={servico.nome} marca={servico.marca} size={32} />
        <HStack gap={8} shrink>
          <Text size="body" weight="semibold" tone="text">{servico.nome}</Text>
          <Text size="callout" tone="secondary">{servico.categoria}</Text>
        </HStack>
      </label>
      {marcado ? (
        <div className="mercado-google-grupos" role="group" aria-label={servico.nome}>
          {f.tipo.grupos.map((g) => (
            <label key={g.id} className="mercado-opcao" aria-disabled={!g.disponivel}>
              <input type="checkbox" checked={f.escolhidos.includes(g.id)} disabled={!g.disponivel} onChange={() => fluxo.alternarGrupo(servico.id, g.id)} />
              <Text size="callout" tone={g.disponivel ? 'text' : 'tertiary'}>{g.rotulo}</Text>
              {g.padrao && g.disponivel && padraoSeguro(f.tipo).includes(g.id) ? <Text size="caption" weight="medium" className="mercado-etiqueta neutra">{t('google.servicos.padrao')}</Text> : null}
              {!g.disponivel ? <Text size="caption" tone="tertiary">{t('google.servicos.emBreve')}</Text> : null}
            </label>
          ))}
          {nomes.pedeNomeDaPrimeira && nomes.primeira ? (
            <div className="mercado-google-nome">
              <input className="mercado-campo" aria-label={t('google.nome.primeira', { servico: servico.nome })} aria-invalid={erroNome('primeira') !== null} value={f.nomeDaPrimeira} placeholder={t('google.nome.primeira', { servico: servico.nome })} onChange={(e) => fluxo.nomear(servico.id, 'primeira', e.target.value)} />
              {erroNome('primeira') ? <Erro>{erroNome('primeira')}</Erro> : null}
            </div>
          ) : null}
          {nomes.pedeNomeDaNova ? (
            <div className="mercado-google-nome">
              <input className="mercado-campo" aria-label={t('google.nome.nova', { servico: servico.nome })} aria-invalid={erroNome('nova') !== null} value={f.nomeDaNova} placeholder={t('google.nome.nova', { servico: servico.nome })} onChange={(e) => fluxo.nomear(servico.id, 'nova', e.target.value)} />
              {erroNome('nova') ? <Erro>{erroNome('nova')}</Erro> : null}
            </div>
          ) : null}
        </div>
      ) : null}
    </div>
  )
}

// ───────────────────────────── Passo 3: o login, um serviço por vez ─────────────────────────────

function Entrar({ fluxo, aoCancelar, aoConcluir }: { fluxo: FluxoGoogle; aoCancelar: () => void; aoConcluir: () => void }) {
  const t = useT()
  const [detalhes, setDetalhes] = useState<string | null>(null)
  const fila = fluxo.servicos.filter((s) => fluxo.marcado(s.id))
  const pronto = fluxo.passo === 'pronto'
  const falhou = fila.some((s) => fluxo.estadoDe(s.id) === 'erro')
  const sinal = (e: ReturnType<FluxoGoogle['estadoDe']>) =>
    e === 'pronto' ? <Icon name="checkmark.circle.fill" size={16} color="var(--success)" />
    : e === 'erro' ? <Icon name="xmark.circle" size={16} />
    : e === 'espera' ? <Icon name="circle" size={16} />
    : <Spinner size={14} />
  return (
    <>
      <div className="mercado-estado" role="status" aria-live="polite">
        <HStack gap={12}>
          <Logos servicos={fila} size={32} />
          <VStack gap={2} shrink>
            <Text as="h3" size="headline" weight="semibold" tone="text">{t('google.entrar.titulo')}</Text>
            <Text size="callout" tone="secondary">{t('google.entrar.sub')}</Text>
          </VStack>
        </HStack>
        <div className="mercado-etapas">
          {fila.map((s) => {
            const e = fluxo.estadoDe(s.id)
            return (
              <div key={s.id} className={`mercado-etapa${e === 'pronto' ? ' feita' : e === 'entrando' || e === 'conferindo' ? ' agora' : ''}`}>
                {sinal(e)}
                <Text size="body">{s.nome}</Text>
                <Text size="callout" tone={e === 'erro' ? 'attention' : 'secondary'}>{tx(t, `google.estado.${e}`)}</Text>
                {e === 'erro' ? <button className="mercado-link discreto" onClick={() => setDetalhes(detalhes === s.id ? null : s.id)}><Text size="callout">{t('mercado.erro.detalhes')}</Text></button> : null}
              </div>
            )
          })}
        </div>
        {fila.filter((s) => detalhes === s.id).map((s) => (
          <div key={s.id} className="mercado-detalhes"><Text size="caption" mono className="mercado-mono">{fluxo.falhaDe(s.id)}</Text></div>
        ))}
        {falhou ? <Aviso icon="info.circle">{t('google.entrar.dicaTipo')}</Aviso> : null}
        <div className="mercado-acoes">
          {pronto ? <Button onClick={aoConcluir}>{t('google.entrar.concluir')}</Button> : null}
          {falhou && !fluxo.ocupado ? <Button onClick={() => void fluxo.tentarDeNovo()}>{t('google.entrar.tentar')}</Button> : null}
          {pronto ? null : <button className="mercado-botao-texto" onClick={aoCancelar}><Text size="body" weight="medium">{t('google.guia.cancelar')}</Text></button>}
        </div>
      </div>
    </>
  )
}
