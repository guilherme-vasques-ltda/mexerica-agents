// Tela 02: a ficha do serviço. Pedidos de exemplo, os jeitos oficiais de conectar, o que dá para liberar, as contas
// deste Space, os links do fornecedor e "Conferida em". Em breve, a ficha só explica.
import { useState } from 'react'
import { Button } from '../../../components/Controls'
import { Icon } from '../../../components/Icon'
import { HStack, VStack } from '../../../components/Stack'
import { Text } from '../../../components/Text'
import { formatarData } from '../../../lib/fork-i18n/index.ts'
import { useIdioma, useT } from '../../../lib/fork-i18n/estado'
import { jeitoEscolhido, jeitoRecomendado, motivosDoEmBreve, perigosasDoTipo } from './regras.ts'
import type { Servico, TipoConexao } from './tipos.ts'
import { Aviso, LogoServico, PontoDeEstado, Titulo } from './pecas'
import { SkillsSugeridas } from './SkillsSugeridas'

const abrirFora = (url: string) => { if (URL.canParse(url)) window.codync.app.openExternal(url) }

/** O nome curto de uma skill pelo endereço dela: o último pedaço do caminho, ou o domínio. */
const nomeDaSkill = (url: string) => {
  if (!URL.canParse(url)) return url
  const { hostname, pathname } = new URL(url)
  const ultimo = pathname.split('/').filter(Boolean).pop()
  return ultimo ? `${hostname.replace(/^www\./, '')} · ${ultimo}` : hostname.replace(/^www\./, '')
}

export function Ficha({ servico, prazoDias, aoVoltar, aoConectar, aoGerenciar }: {
  servico: Servico
  /** Os dias que o Hub dá a uma conferência (90). */
  prazoDias: number
  aoVoltar: () => void
  aoConectar?: (tipo: TipoConexao) => void
  aoGerenciar: () => void
}) {
  const t = useT()
  const idioma = useIdioma()
  const [escolhido, setEscolhido] = useState<string | null>(jeitoRecomendado(servico)?.id ?? null)
  const [detalhes, setDetalhes] = useState(false)
  // O jeito marcado manda em tudo: Conectar, "O que dá para liberar", ações perigosas e detalhes técnicos.
  const tipo = jeitoEscolhido(servico, escolhido)
  const contas = servico.contas ?? []
  const perigosas = perigosasDoTipo(tipo)
  const data = Number.isNaN(Date.parse(`${servico.conferidaEm}T12:00:00`)) ? servico.conferidaEm : formatarData(idioma, new Date(`${servico.conferidaEm}T12:00:00`), { dateStyle: 'short' })
  const liberaveis = tipo?.grupos ?? []
  const pode = servico.emBreve === null && servico.conectavel && tipo !== null

  return (
    <div className="mercado">
      <button className="mercado-voltar" onClick={aoVoltar}>
        <Icon name="arrow.left" size={12} /><Text size="callout">{t('mercado.ficha.voltar')}</Text>
      </button>
      <div className="mercado-ficha">
        <div className="mercado-ficha-principal">
          <div className="mercado-cabeca">
            <LogoServico nome={servico.nome} marca={servico.marca} size={56} />
            <VStack gap={4} shrink>
              <Text as="h2" size="title" weight="semibold" tone="text">{servico.nome}</Text>
              <HStack gap={6}>
                {servico.tipos.some((x) => x.tipo.startsWith('oficial')) ? <><Icon name="checkmark.seal" size={12} /><Text size="callout" tone="secondary">{t('mercado.selo.oficial')} · {servico.categoria}</Text></> : <Text size="callout" tone="secondary">{servico.categoria}</Text>}
              </HStack>
              <Text size="body" tone="secondary">{servico.descricao}</Text>
            </VStack>
          </div>

          {servico.aviso ? <Aviso icon="exclamationmark.triangle" alerta>{servico.aviso}</Aviso> : null}

          {servico.emBreve !== null ? (
            <VStack gap={8}>
              <Text size="body" tone="text">{t('mercado.embreve.texto', { servico: servico.nome })}</Text>
              <section className="mercado-secao mercado-porque" aria-label={t('mercado.embreve.porque')}>
                <Text as="h3" size="callout" weight="semibold" tone="secondary">{t('mercado.embreve.porque')}</Text>
                {motivosDoEmBreve(servico).map((m) => <Text key={m} size="body" tone="text">{m}</Text>)}
              </section>
              <Text size="callout" tone="secondary">{t('mercado.embreve.depois')}</Text>
            </VStack>
          ) : (
            <>
              {aoConectar && servico.exemplos.length > 0 ? (
                <section className="mercado-secao">
                  <Titulo titulo={t('mercado.ficha.pedir.titulo')} sub={t('mercado.ficha.pedir.sub')} />
                  <div className="mercado-exemplos">
                    {servico.exemplos.map((e) => <div key={e} className="mercado-balao"><Text size="body" tone="text">{e}</Text></div>)}
                  </div>
                </section>
              ) : null}

              {aoConectar ? <section className="mercado-secao">
                <Titulo titulo={t('mercado.ficha.como.titulo')} sub={t('mercado.ficha.como.sub')} />
                {servico.tipos.length === 0 ? <Text size="callout" tone="secondary">{t('mercado.ficha.semJeito')}</Text> : (
                  <div className="mercado-jeitos" role="radiogroup" aria-label={t('mercado.ficha.como.titulo')}>
                    {servico.tipos.map((x) => (
                      <button key={x.id} role="radio" aria-checked={x.id === escolhido} disabled={!x.disponivel} className="mercado-jeito" onClick={() => setEscolhido(x.id)}>
                        <span className="mercado-marca" aria-hidden />
                        <VStack gap={2} shrink>
                          <HStack gap={8}>
                            <Text size="body" weight="semibold" tone="text">{x.rotulo}</Text>
                            {x.recomendado ? <Text size="caption" weight="semibold" className="mercado-etiqueta">{t('mercado.ficha.recomendado')}</Text> : null}
                          </HStack>
                          <Text size="callout" tone="secondary">
                            {x.disponivel
                              ? (x.auth.modo === 'oauth' ? t('mercado.ficha.jeito.oauth', { servico: servico.nome }) : x.auth.modo === 'app_proprio' ? t('google.grupo.contas') : `${t('mercado.ficha.jeito.chave', { servico: servico.nome })} ${x.auth.comoCriarLeitura}`.trim())
                              : (x.motivoIndisponivel ?? t('mercado.lib.indisponivel'))}
                          </Text>
                        </VStack>
                      </button>
                    ))}
                  </div>
                )}
                {!servico.conferenciaVigente ? <Aviso icon="exclamationmark.triangle" alerta>{t('mercado.ficha.vencida', { dias: prazoDias })}</Aviso> : null}
                <HStack gap={12}>
                  <Button onClick={() => tipo && aoConectar(tipo)} disabled={!pode}>
                    {t(contas.length > 0 ? 'mercado.ficha.conectarOutra' : 'mercado.ficha.conectar', { servico: servico.nome })}
                  </Button>
                  <Text size="callout" tone="secondary">{t('mercado.ficha.proximo')}</Text>
                </HStack>
              </section> : null}
            </>
          )}
        </div>

        <aside className="mercado-ficha-lateral">
          {aoConectar ? <section className="mercado-secao">
            <Text as="h3" size="callout" weight="semibold" tone="secondary">{t('mercado.ficha.contas.titulo')}</Text>
            {contas.length === 0 ? <Text size="callout" tone="secondary">{t('mercado.ficha.contas.nenhuma', { servico: servico.nome })}</Text> : (
              <div className="mercado-lista-fina">
                {contas.map((c) => (
                  <div key={c.id} className="mercado-linha">
                    <VStack gap={0} grow shrink>
                      <Text size="body" weight="medium" tone="text" truncate>{c.name}</Text>
                      {c.identidade ? <Text size="callout" tone="secondary" truncate>{c.identidade}</Text> : null}
                    </VStack>
                    <PontoDeEstado />
                  </div>
                ))}
              </div>
            )}
            {contas.length > 0 ? <button className="mercado-link" onClick={aoGerenciar}><HStack gap={4}><Text size="callout" weight="semibold">{t('mercado.ficha.contas.gerenciar')}</Text><Icon name="chevron.right" size={10} /></HStack></button> : null}
          </section> : null}

          {aoConectar && liberaveis.length > 0 ? (
            <section className="mercado-secao">
              <Text as="h3" size="callout" weight="semibold" tone="secondary">{t('mercado.ficha.liberar.titulo')}</Text>
              <div className="mercado-lista-fina">
                {liberaveis.map((g) => (
                  <div key={g.id} className="mercado-linha">
                    <Icon name={g.padrao ? 'checkmark' : 'pencil'} size={12} />
                    <Text size="body" tone={g.disponivel ? 'text' : 'tertiary'} grow>{g.rotulo}</Text>
                    {g.padrao ? <Text size="caption" weight="medium" className="mercado-etiqueta neutra">{t('mercado.ficha.liberar.padrao')}</Text> : null}
                    {!g.disponivel ? <Text size="caption" tone="tertiary">{t('mercado.lib.indisponivel')}</Text> : null}
                  </div>
                ))}
                {perigosas.length > 0 ? (
                  <div className="mercado-linha">
                    <Icon name="exclamationmark.triangle" size={12} />
                    <VStack gap={0} shrink>
                      <Text size="body" tone="text">{t('mercado.ficha.perigosas.titulo', { lista: perigosas.map((p) => p.rotulo).join(', ') })}</Text>
                      <Text size="caption" tone="secondary">{t('mercado.ficha.perigosas.nota')}</Text>
                    </VStack>
                  </div>
                ) : null}
              </div>
            </section>
          ) : null}

          {aoConectar ? <SkillsSugeridas servico={servico} emBreve={servico.emBreve !== null} /> : null}

          {aoConectar && (servico.skillsOficiais ?? []).length > 0 ? (
            <section className="mercado-secao">
              <Text as="h3" size="callout" weight="semibold" tone="secondary">{t('mercado.ficha.skills.titulo')}</Text>
              <div className="mercado-lista-fina">
                {(servico.skillsOficiais ?? []).map((s) => (
                  <button key={s.url} className="mercado-link" onClick={() => abrirFora(s.url)}>
                    <HStack gap={4}><Text size="callout" truncate>{nomeDaSkill(s.url)}</Text><Text size="caption" tone="tertiary">{t('mercado.ficha.skills.licenca', { licenca: s.licenca })}</Text><Icon name="arrow.up.right.square" size={11} /></HStack>
                  </button>
                ))}
              </div>
            </section>
          ) : null}

          <section className="mercado-secao">
            <Text as="h3" size="callout" weight="semibold" tone="secondary">{t('mercado.ficha.fornecedor.titulo')}</Text>
            {aoConectar ? <>
            <button className="mercado-link" onClick={() => abrirFora(servico.links.privacidade)}><HStack gap={4}><Text size="callout">{t('mercado.ficha.privacidade', { servico: servico.nome })}</Text><Icon name="arrow.up.right.square" size={11} /></HStack></button>
            <button className="mercado-link" onClick={() => abrirFora(servico.links.termos)}><HStack gap={4}><Text size="callout">{t('mercado.ficha.termos', { servico: servico.nome })}</Text><Icon name="arrow.up.right.square" size={11} /></HStack></button>
            {servico.site ? <button className="mercado-link" onClick={() => abrirFora(servico.site!)}><HStack gap={4}><Text size="callout">{t('mercado.ficha.site', { servico: servico.nome })}</Text><Icon name="arrow.up.right.square" size={11} /></HStack></button> : null}
            {servico.docs ? <button className="mercado-link" onClick={() => abrirFora(servico.docs!)}><HStack gap={4}><Text size="callout">{t('mercado.ficha.docs')}</Text><Icon name="arrow.up.right.square" size={11} /></HStack></button> : null}
            </> : null}
            <Text size="caption" tone="secondary">{t('mercado.ficha.conferida', { data })}</Text>
            {tipo ? (
              <>
                <button className="mercado-link" aria-expanded={detalhes} onClick={() => setDetalhes(!detalhes)}>
                  <HStack gap={4}><Text size="callout" weight="semibold">{t(detalhes ? 'mercado.ficha.detalhes.ocultar' : 'mercado.ficha.detalhes.ver')}</Text><Icon name={detalhes ? 'chevron.up' : 'chevron.down'} size={10} /></HStack>
                </button>
                {detalhes ? (
                  <div className="mercado-detalhes">
                    <Text size="caption" weight="semibold" tone="secondary">{t('mercado.ficha.detalhes.endereco')}</Text>
                    <Text size="caption" mono className="mercado-mono">{tipo.url}</Text>
                    {tipo.grupos.flatMap((g) => g.escopos).length > 0 ? (
                      <>
                        <Text size="caption" weight="semibold" tone="secondary">{t('mercado.ficha.detalhes.escopos')}</Text>
                        <Text size="caption" mono className="mercado-mono">{tipo.grupos.flatMap((g) => g.escopos).join(' ')}</Text>
                      </>
                    ) : null}
                  </div>
                ) : null}
              </>
            ) : null}
          </section>
        </aside>
      </div>
    </div>
  )
}
