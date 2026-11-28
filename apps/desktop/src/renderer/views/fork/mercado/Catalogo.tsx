// Tela 01: o catálogo. Busca, categorias, a lista com selos discretos e o contador de contas,
// e o bloco "Em breve" sem data. Só renderiza o que o Hub mandou; a escolha do que mostrar vem de regras.ts.
import { useMemo, useState } from 'react'
import { SearchField } from '../../../components/Controls'
import { Icon } from '../../../components/Icon'
import { VStack } from '../../../components/Stack'
import { Text } from '../../../components/Text'
import { useT } from '../../../lib/fork-i18n/estado'
import { categoriasDoCatalogo, contarPorCategoria, estadoDoServico, seloDoServico, visaoDoCatalogo } from './regras.ts'
import type { AppGoogle, Catalogo as CatalogoDoHub, Conta, Servico } from './tipos.ts'
import { GrupoGoogle } from './GrupoGoogle'
import { Aviso, LogoServico, PontoDeEstado } from './pecas'

export function Catalogo({ catalogo, contas, cofreTrancado, contasFalharam, appGoogle, aoTentar, aoAbrir, aoAbrirGoogle }: {
  catalogo: CatalogoDoHub
  contas: readonly Conta[]
  /** O Hub disse que o cofre está trancado: as contas não vêm, o catálogo sim. */
  cofreTrancado: boolean
  /** `marketContas` falhou sem o Hub dizer o porquê: aviso genérico com "Tentar de novo". */
  contasFalharam: boolean
  /** O app Google do Space (só o ID); `null` = sem app ainda, ou cofre trancado. */
  appGoogle: AppGoogle | null
  aoTentar: () => void
  aoAbrir: (servico: Servico) => void
  /** Abre o fluxo do Google: o guia quando falta o app, senão a escolha dos serviços. */
  aoAbrirGoogle?: () => void
}) {
  const t = useT()
  const [busca, setBusca] = useState('')
  const [categoria, setCategoria] = useState<string | null>(null)
  const categorias = useMemo(() => categoriasDoCatalogo(catalogo.servicos, catalogo.categorias), [catalogo])
  const contagem = useMemo(() => contarPorCategoria(catalogo.servicos), [catalogo])
  const visao = useMemo(() => visaoDoCatalogo(catalogo, busca, categoria), [catalogo, busca, categoria])
  const vazio = visao.google.length === 0 && visao.lista.length === 0 && visao.emBreve.length === 0

  return (
    <div className="mercado">
      {cofreTrancado ? <Aviso icon="lock">{t('mercado.cofre.trancado')}</Aviso> : null}
      {contasFalharam && !cofreTrancado ? (
        <Aviso icon="exclamationmark.triangle" alerta>
          {t('mercado.contas.falhou')}{' '}
          <button className="mercado-link" onClick={aoTentar}><Text size="callout" weight="semibold">{t('mercado.catalogo.tentar')}</Text></button>
        </Aviso>
      ) : null}
      <SearchField value={busca} onChange={setBusca} prompt={t('mercado.busca.placeholder')} clearLabel={t('mercado.busca.limpar')} />
      <div className="mercado-categorias" role="group" aria-label={t('mercado.cat.rotulo')}>
        {[null, ...categorias].map((c) => (
          <button key={c ?? 'todos'} className="mercado-categoria" aria-pressed={categoria === c} onClick={() => setCategoria(c)}>
            <Text size="callout" weight="medium">{c ?? t('mercado.cat.todos')}</Text>
            <Text size="caption" tabular className="mercado-categoria-n">{c === null ? catalogo.servicos.length : contagem[c]}</Text>
          </button>
        ))}
      </div>

      {visao.google.length > 0 ? aoAbrirGoogle
        ? <GrupoGoogle servicos={visao.google} app={appGoogle} contas={contas} cofreTrancado={cofreTrancado} aoAbrir={aoAbrirGoogle} />
        : <div className="mercado-grade">{visao.google.map((s) => <CartaoDoServico key={s.id} servico={s} contas={contas} aoAbrir={aoAbrir} soLeitura />)}</div>
        : null}

      {visao.lista.length > 0 ? (
        <section className="mercado-secao" aria-label={t('mercado.lista.titulo')}>
          <div className="mercado-secao-topo">
            <Text as="h3" size="headline" weight="semibold" tone="text">{categoria ?? t('mercado.lista.titulo')}</Text>
            <Text size="callout" tone="secondary" tabular>{t.plural('mercado.lista.total', visao.total)}</Text>
          </div>
          {visao.porCategoria.map((grupo) => (
            <div key={grupo.categoria} className="mercado-categoria-bloco" role="group" aria-label={grupo.categoria}>
              {visao.porCategoria.length > 1 ? (
                <div className="mercado-categoria-titulo">
                  <Text as="div" size="callout" weight="semibold" tone="secondary">{grupo.categoria}</Text>
                  <Text size="caption" tone="tertiary" tabular>{grupo.servicos.length}</Text>
                </div>
              ) : null}
              <div className="mercado-grade">
                {grupo.servicos.map((s) => <CartaoDoServico key={s.id} servico={s} contas={contas} aoAbrir={aoAbrir} soLeitura={!aoAbrirGoogle} />)}
              </div>
            </div>
          ))}
        </section>
      ) : null}

      {vazio ? (
        <VStack align="center" gap={8} className="mercado-vazio">
          <Icon name="magnifyingglass" size={26} color="var(--text-tertiary, var(--ink-3))" />
          <Text size="compactBody" weight="semibold" tone="text">{t('mercado.vazio.busca.titulo', { busca: busca.trim() })}</Text>
          <Text size="callout" tone="secondary">{t('mercado.vazio.busca.dica')}</Text>
        </VStack>
      ) : null}

      {visao.emBreve.length > 0 ? (
        <section className="mercado-secao" aria-label={t('mercado.embreve.titulo')}>
          <div className="mercado-secao-topo">
            <Text as="h3" size="headline" weight="semibold" tone="text">{t('mercado.embreve.titulo')}</Text>
            <Text size="callout" tone="secondary">{t('mercado.embreve.sub')}</Text>
          </div>
          <div className="mercado-embreve">
            {visao.emBreve.map((s) => (
              <button key={s.id} className="mercado-embreve-item" onClick={() => aoAbrir(s)} aria-label={t('mercado.card.abrir', { servico: s.nome })}>
                <LogoServico nome={s.nome} marca={s.marca} size={28} />
                <VStack gap={0} shrink>
                  <Text size="body" weight="medium" tone="text" truncate>{s.nome}</Text>
                  <Text size="caption" tone="secondary" truncate>{s.categoria}</Text>
                </VStack>
              </button>
            ))}
          </div>
        </section>
      ) : null}
    </div>
  )
}

function SelosDoServico({ servico }: { servico: Servico }) {
  const t = useT()
  const selo = seloDoServico(servico)
  const vencida = !servico.conferenciaVigente
  return (
    <span className={`mercado-selos${vencida ? ' alerta' : ''}`}>
      {selo.oficial ? <Icon name="checkmark.seal" size={11} /> : null}
      <Text size="caption">
        {[
          selo.oficial ? t('mercado.selo.oficial') : null,
          selo.jeitos > 1 ? t.plural('mercado.selo.jeitos', selo.jeitos) : null,
          vencida ? t('mercado.selo.vencida') : !servico.conectavel ? t('mercado.selo.indisponivel') : null,
        ].filter(Boolean).join(' · ')}
      </Text>
    </span>
  )
}

function CartaoDoServico({ servico, contas, aoAbrir, soLeitura }: { servico: Servico; contas: readonly Conta[]; aoAbrir: (s: Servico) => void; soLeitura: boolean }) {
  const t = useT()
  const estado = estadoDoServico(servico, contas)
  return (
    <button className="mercado-card" onClick={() => aoAbrir(servico)} aria-label={t('mercado.card.abrir', { servico: servico.nome })}>
      <LogoServico nome={servico.nome} marca={servico.marca} size={44} />
      <span className="mercado-card-texto">
        <Text size="body" weight="semibold" tone="text" truncate>{servico.nome}</Text>
        <Text size="body" tone="secondary" lines={1}>{servico.descricao}</Text>
        <SelosDoServico servico={servico} />
      </span>
      <span className="mercado-card-fim">
        {soLeitura ? <Icon name="chevron.right" size={11} /> : estado.tipo === 'semConta' ? (
          <span className="mercado-capsula"><Text size="callout" weight="medium">{t('mercado.acao.conectar')}</Text></span>
        ) : (
          <>
            <PontoDeEstado alerta={estado.tipo === 'reconectar'} />
            <Text size="callout" tone={estado.tipo === 'reconectar' ? 'attention' : 'secondary'}>
              {estado.tipo === 'reconectar' ? t('mercado.acao.reconectar') : t.plural('mercado.contas.n', estado.n)}
            </Text>
            <Icon name="chevron.right" size={11} />
          </>
        )}
      </span>
    </button>
  )
}
