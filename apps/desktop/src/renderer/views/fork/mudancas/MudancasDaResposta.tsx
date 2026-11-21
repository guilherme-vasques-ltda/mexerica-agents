import { useState } from 'react'
import { CardSection, Disclosure } from '../../../components/Controls'
import { Icon } from '../../../components/Icon'
import { ModalHeader, SheetPage } from '../../../components/Overlay'
import { HStack, VStack } from '../../../components/Stack'
import { Text } from '../../../components/Text'
import { superficieIdioma, tradutor, useIdioma, useT } from '../../../lib/fork-i18n/estado'
import { DiffView } from '../../thread/PermissionCard'
import { stepText } from '../../thread/cards/step-text'
import { shouldShowChangesTrigger, type FileChangeGroup, type TurnChanges } from './mudancas'
import './mudancas.css'

export function ChangesTrigger({ changes, onOpen }: { changes: TurnChanges; onOpen: () => void }) {
  const t = useT()
  if (!shouldShowChangesTrigger(changes)) return null

  const { files, approxAdded, approxRemoved, gaps } = changes
  const hasFiles = files.length > 0
  const hasGaps = gaps.partial || gaps.untrackedCommandCount > 0 || gaps.historyIncomplete

  const label = hasFiles
    ? t.plural('conversa.mudancas.gatilho.arquivos', files.length)
    : gaps.untrackedCommandCount > 0
      ? t('conversa.mudancas.gatilho.semRegistro')
      : t('conversa.mudancas.gatilho.historicoIndisponivel')

  const resumo = hasFiles
    ? t('conversa.mudancas.gatilho.resumoAprox', {
      rotulo: label,
      adicionadas: approxAdded,
      removidas: approxRemoved,
    })
    : label

  const lacunas: string[] = []
  if (gaps.historyIncomplete) lacunas.push(t('conversa.mudancas.gatilho.lacunaHistorico'))
  if (gaps.untrackedCommandCount > 0) lacunas.push(t.plural('conversa.mudancas.gatilho.lacunaSemRegistro', gaps.untrackedCommandCount))
  if (gaps.partial) lacunas.push(t('conversa.mudancas.gatilho.lacunaParcial'))

  const ariaLabel = lacunas.length > 0
    ? t('conversa.mudancas.gatilho.ariaComLacunas', {
      resumo,
      lacunas: lacunas.join(t('conversa.mudancas.gatilho.separadorLacuna')),
    })
    : t('conversa.mudancas.gatilho.aria', { resumo })

  return (
    <button type="button" className="mudancas-trigger" aria-label={ariaLabel} onClick={onOpen}>
      <Icon name={hasFiles ? 'doc.text' : 'exclamationmark.triangle'} size={11} className="mudancas-trigger-icon" />
      <span className="mudancas-trigger-label">{label}</span>
      {hasFiles ? (
        <span className="mudancas-trigger-delta codigo-ltr" dir="ltr">
          <span className="mudancas-delta-add">{`~+${approxAdded}`}</span>
          {' '}
          <span className="mudancas-delta-rem">{`−${approxRemoved}`}</span>
        </span>
      ) : null}
      {hasFiles && hasGaps ? (
        <span className="mudancas-trigger-flag" aria-hidden="true">
          <Icon name="exclamationmark.triangle" size={11} color="var(--warning)" />
        </span>
      ) : null}
    </button>
  )
}

export function MudancasDaResposta({ changes }: { changes: TurnChanges }) {
  const idioma = useIdioma()
  const t = tradutor(idioma)
  const { files, approxAdded, approxRemoved, gaps } = changes
  const hasGaps = gaps.historyIncomplete || gaps.untrackedCommandCount > 0 || gaps.partial
  const totalHunks = files.reduce((sum, file) => sum + file.hunks.length, 0)
  const defaultOpen = files.length <= 3

  return (
    <SheetPage>
      <ModalHeader closeTitle={t('conversa.fechar')} title={t('conversa.mudancas.titulo')} />
      <div className="thread-scroll idioma-superficie" {...superficieIdioma(idioma)}>
        <VStack gap={14} className="mudancas-content">
          <div className="mudancas-banner" role="note">
            <Icon name="info.circle" size={14} className="mudancas-banner-icon" />
            <VStack gap={2} grow>
              <Text as="div" size="subheadline" weight="semibold" tone="text">{t('conversa.mudancas.aviso.titulo')}</Text>
              <Text as="div" size="footnote" tone="secondary">{t('conversa.mudancas.aviso.corpo')}</Text>
            </VStack>
          </div>

          {hasGaps ? (
            <section className="mudancas-gaps" role="region" aria-label={t('conversa.mudancas.lacunas.titulo')}>
              <Text as="h3" size="subheadline" weight="semibold" tone="attention">{t('conversa.mudancas.lacunas.titulo')}</Text>
              <ul className="mudancas-gaps-list">
                {gaps.historyIncomplete ? (
                  <li className="mudancas-gap-item">
                    <Icon name="exclamationmark.triangle" size={12} color="var(--warning)" className="mudancas-gap-icon" />
                    <Text size="footnote" tone="text">{t('conversa.mudancas.lacuna.historico')}</Text>
                  </li>
                ) : null}
                {gaps.untrackedCommandCount > 0 ? (
                  <li className="mudancas-gap-item">
                    <Icon name="exclamationmark.triangle" size={12} color="var(--warning)" className="mudancas-gap-icon" />
                    <Text size="footnote" tone="text">{t.plural('conversa.mudancas.lacuna.semRegistro', gaps.untrackedCommandCount)}</Text>
                  </li>
                ) : null}
                {gaps.partial ? (
                  <li className="mudancas-gap-item">
                    <Icon name="exclamationmark.triangle" size={12} color="var(--warning)" className="mudancas-gap-icon" />
                    <Text size="footnote" tone="text">{t('conversa.mudancas.lacuna.parcial')}</Text>
                  </li>
                ) : null}
              </ul>
            </section>
          ) : null}

          {files.length > 0 ? (
            <>
              <HStack gap={8} justify="space-between" className="mudancas-summary-bar">
                <Text size="footnote" weight="medium" tone="secondary">
                  {t('conversa.mudancas.resumo.linha', {
                    arquivos: t.plural('conversa.mudancas.resumo.arquivos', files.length),
                    trechos: t.plural('conversa.mudancas.resumo.trechos', totalHunks),
                  })}
                </Text>
                <span className="mudancas-delta codigo-ltr" dir="ltr">
                  <Text size="footnote" weight="medium" tone="added" tabular>{`~+${approxAdded}`}</Text>
                  {' '}
                  <Text size="footnote" weight="medium" tone="removed" tabular>{`−${approxRemoved}`}</Text>
                </span>
              </HStack>
              <VStack gap={12}>
                {files.map((file) => (
                  <FileGroupCard key={`${changes.replyId}:${file.path}`} file={file} defaultOpen={defaultOpen} />
                ))}
              </VStack>
            </>
          ) : (
            <CardSection>
              <VStack gap={4} className="mudancas-empty">
                <Text as="div" size="subheadline" weight="semibold" tone="text">{t('conversa.mudancas.vazio.titulo')}</Text>
                <Text as="div" size="footnote" tone="secondary">{t('conversa.mudancas.vazio.corpo')}</Text>
              </VStack>
            </CardSection>
          )}
        </VStack>
      </div>
    </SheetPage>
  )
}

function FileGroupCard({ file, defaultOpen }: { file: FileChangeGroup; defaultOpen: boolean }) {
  const [expanded, setExpanded] = useState(defaultOpen)
  const t = useT()

  const label = (
    <HStack as="span" gap={8} grow className="mudancas-file-label">
      <Icon
        name={file.isNew ? 'doc.badge.plus' : 'doc.text'}
        size={12}
        color="var(--text-secondary, var(--ink-2))"
        title={t(file.isNew ? 'conversa.mudancas.arquivo.novo' : 'conversa.mudancas.arquivo.editado')}
      />
      <bdi dir="ltr" className="codigo-ltr mudancas-file-path" title={file.path}>
        {file.path}
      </bdi>
      <span className="mudancas-delta codigo-ltr" dir="ltr">
        <Text size="caption" tone="added" tabular>{`~+${file.approxAdded}`}</Text>
        {' '}
        <Text size="caption" tone="removed" tabular>{`−${file.approxRemoved}`}</Text>
      </span>
      {file.hunks.length > 1 ? (
        <span className="mudancas-badge">{t.plural('conversa.mudancas.arquivo.trechos', file.hunks.length)}</span>
      ) : null}
      {file.hasTruncated ? (
        <span className="mudancas-badge mudancas-badge-warn">{t('conversa.mudancas.selo.resumido')}</span>
      ) : null}
    </HStack>
  )

  return (
    <CardSection>
      <div className="mudancas-file-body">
        <Disclosure expanded={expanded} onToggle={() => setExpanded((open) => !open)} label={label}>
          <VStack gap={10} className="mudancas-hunks">
            {file.hunks.map((hunk, index) => (
              <div key={`${hunk.entryId}-${hunk.stepIndex}-${index}`} className="mudancas-hunk">
                <HStack gap={8} className="mudancas-hunk-meta">
                  <Text size="caption" weight="medium" tone="secondary" truncate grow title={hunk.toolTitle || undefined}>
                    {stepText(t, { title: hunk.toolTitle, kind: hunk.toolKind })}
                  </Text>
                  <bdi dir="ltr" className="codigo-ltr mudancas-line-badge">
                    <Text size="caption" mono tone="tertiary" tabular>{t('conversa.mudancas.trecho.linha', { linha: hunk.diff.startLine })}</Text>
                  </bdi>
                  {hunk.truncated ? (
                    <span className="mudancas-badge mudancas-badge-warn">{t('conversa.mudancas.selo.resumido')}</span>
                  ) : null}
                </HStack>
                <DiffView diff={hunk.diff} />
              </div>
            ))}
          </VStack>
        </Disclosure>
      </div>
    </CardSection>
  )
}
