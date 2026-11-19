import { Fragment, useMemo, useState, type ReactNode } from 'react'
import { FILE_CHUNK_SIZE } from '@shared/files'
import { Icon } from '../../../components/Icon'
import { ModalHeader, Sheet } from '../../../components/Overlay'
import { Text } from '../../../components/Text'
import { cellLines, numericColumns, toCSV, toTSV, visibleRows, type NumberReader, type SortDir } from './data-table'
import { formatarNumero } from '../../../lib/fork-i18n'
import { superficieIdioma, useIdioma, tradutor } from '../../../lib/fork-i18n/estado'
import './data-table.css'

// D165 fase F, D169 (components/DataTable): a table the bot sent in Markdown, with search over every
// row, sort by column, wrapped lines (on by default), copy as TSV, CSV through the app's save dialog,
// and expand to a sheet (Esc returns). It only reads the message's own table.

type Render = (text: string, key: string) => ReactNode
// D273: optional hooks for the chart card's Ver dados.
// `plain` keeps each cell literal (no `<br>` split); `parseNumber` replaces the Markdown number heuristic.
// Both absent = today's Markdown table.
interface Options { plain?: boolean; parseNumber?: NumberReader }

/** D249 R6: up to this many rows, the table in the bubble has only Copiar and Expandir (in its footer); search,
 * wrap and CSV stay in the expanded sheet, where a table of any size has them. */
export const SHORT_TABLE_ROWS = 8

export function DataTable({ header, rows, renderCell, plain, parseNumber }: { header: string[]; rows: string[][]; renderCell: Render } & Options) {
  const [expanded, setExpanded] = useState(false)
  const idioma = useIdioma()
  const t = tradutor(idioma)
  const options = { plain, parseNumber }
  return (
    <>
      <TableView header={header} rows={rows} renderCell={renderCell} onExpand={() => setExpanded(true)} {...options} />
      <Sheet language={superficieIdioma(idioma)} open={expanded} onClose={() => setExpanded(false)} width={Math.min(1100, window.innerWidth - 80)} height={Math.min(760, window.innerHeight - 80)} label={t('conversa.cartoes.tabela.titulo')}>
        <div className="data-table-sheet idioma-superficie" {...superficieIdioma(idioma)}>
          <ModalHeader closeTitle={t('conversa.fechar')} title={t('conversa.cartoes.tabela.titulo')} subtitle={t('conversa.cartoes.tabela.tamanho', { linhas: t.plural('conversa.cartoes.tabela.linhas', rows.length), colunas: t.plural('conversa.cartoes.tabela.colunas', header.length) })} />
          {expanded ? <TableView header={header} rows={rows} renderCell={renderCell} fill {...options} /> : null}
        </div>
      </Sheet>
    </>
  )
}

/** A cell's text through the inline renderer, its `<br>` as real line breaks (never as HTML). */
function Cell({ text, id, renderCell, plain }: { text: string; id: string; renderCell: Render; plain?: boolean }) {
  if (plain) return <>{text}</>
  const lines = cellLines(text)
  if (lines.length === 1) return <>{renderCell(text, id)}</>
  return <>{lines.map((line, i) => <Fragment key={i}>{i > 0 ? <br /> : null}{renderCell(line, `${id}-${i}`)}</Fragment>)}</>
}

function TableView({ header, rows, renderCell, onExpand, fill = false, plain, parseNumber }: { header: string[]; rows: string[][]; renderCell: Render; onExpand?: () => void; fill?: boolean } & Options) {
  const columns = Math.max(header.length, ...rows.map((r) => r.length))
  const cols = Array.from({ length: columns }, (_, i) => i)
  const numeric = useMemo(() => numericColumns(rows, columns, parseNumber), [rows, columns, parseNumber])
  const idioma = useIdioma()
  const t = tradutor(idioma)
  const [query, setQuery] = useState('')
  const [sort, setSort] = useState<{ column: number; dir: SortDir } | null>(null)
  const [wrap, setWrap] = useState(true)
  const [status, setStatus] = useState<string | null>(null)
  const shown = useMemo(() => visibleRows(rows, query, sort, numeric, parseNumber), [rows, query, sort, numeric, parseNumber])
  const fullHeader = cols.map((i) => header[i] ?? '')
  const short = !fill && rows.length <= SHORT_TABLE_ROWS

  const cycle = (column: number) => setSort((s) => (s?.column !== column ? { column, dir: 'asc' } : s.dir === 'asc' ? { column, dir: 'desc' } : null))
  const copy = () => {
    window.codync.app.copy(toTSV(fullHeader, shown, plain))
    setStatus(t('conversa.cartoes.tabela.copiada'))
  }
  const download = async () => {
    setStatus(null)
    const bytes = new TextEncoder().encode('﻿' + toCSV(fullHeader, shown, plain))
    const digest = new Uint8Array(await crypto.subtle.digest('SHA-256', bytes))
    const sha256 = [...digest].map((b) => b.toString(16).padStart(2, '0')).join('')
    let handle: string | null = null
    try {
      handle = await window.codync.files.begin({ id: crypto.randomUUID(), name: t('conversa.cartoes.tabela.arquivo'), size: bytes.length, sha256 })
      if (!handle) return
      for (let offset = 0; offset < bytes.length; offset += FILE_CHUNK_SIZE) await window.codync.files.write(handle, offset, bytes.subarray(offset, offset + FILE_CHUNK_SIZE))
      await window.codync.files.finish(handle)
      setStatus(t('conversa.cartoes.tabela.csvSalvo'))
    } catch (e) {
      if (handle) void window.codync.files.cancel(handle).catch(() => {})
      setStatus(t('conversa.cartoes.tabela.csvFalhou', { erro: e instanceof Error ? e.message : String(e) }))
    }
  }

  return (
    // R2 (fatia 2): a barra e o rodapé são traduzidos (idioma e direção do app); as células são dado da mensagem (`dir="auto"`).
    <div className={`data-table idioma-superficie${fill ? ' fill' : ''}`} {...superficieIdioma(idioma)}>
      {short ? null : <div className="data-table-bar">
        <label className="data-table-search">
          <Icon name="magnifyingglass" size={13} color="var(--context-ink-2, var(--ink-2))" />
          <input dir="auto" value={query} onChange={(e) => setQuery(e.currentTarget.value)} placeholder={t('conversa.cartoes.tabela.buscar')} aria-label={t('conversa.cartoes.tabela.buscarAria')} />
        </label>
        <button className="data-table-tool" aria-pressed={wrap} title={t(wrap ? 'conversa.cartoes.tabela.naoQuebrar' : 'conversa.cartoes.tabela.quebrar')} aria-label={t('conversa.cartoes.tabela.quebrar')} onClick={() => setWrap((w) => !w)}>
          <Icon name="text.alignleft" size={15} />
        </button>
        <button className="data-table-tool" title={t('conversa.cartoes.tabela.copiar')} aria-label={t('conversa.cartoes.tabela.copiarAria')} onClick={copy}>
          <Icon name="doc.on.doc" size={15} />
        </button>
        <button className="data-table-tool" title={t('conversa.cartoes.tabela.baixar')} aria-label={t('conversa.cartoes.tabela.baixar')} onClick={() => void download()}>
          <Icon name="square.and.arrow.down" size={15} />
        </button>
        {onExpand ? (
          <button className="data-table-tool" title={t('conversa.cartoes.tabela.expandir')} aria-label={t('conversa.cartoes.tabela.expandirAria')} onClick={onExpand}>
            <Icon name="arrow.up.left.and.arrow.down.right" size={14} />
          </button>
        ) : null}
      </div>}
      <div className="data-table-scroll" tabIndex={0} role="region" aria-label={t('conversa.cartoes.tabela.regiao')}>
        <table className={wrap ? 'wrap' : undefined}>
          <thead>
            <tr>
              {cols.map((i) => (
                <th key={i} scope="col" className={numeric[i] ? 'num' : undefined} aria-sort={sort?.column === i ? (sort.dir === 'asc' ? 'ascending' : 'descending') : 'none'}>
                  <button className="data-table-sort" onClick={() => cycle(i)} title={t('conversa.cartoes.tabela.ordenar')}>
                    <span dir="auto"><Cell text={header[i] ?? ''} id={`h${i}`} renderCell={renderCell} plain={plain} /></span>
                    <span className="data-table-sort-icon" aria-hidden>
                      <Icon name={sort?.column === i ? (sort.dir === 'asc' ? 'arrow.up' : 'arrow.down') : 'arrow.up.arrow.down'} size={11} />
                    </span>
                  </button>
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {shown.map((row, r) => (
              <tr key={r}>
                {cols.map((i) => <td key={i} dir="auto" className={numeric[i] ? 'num' : undefined}><Cell text={row[i] ?? ''} id={`${r}-${i}`} renderCell={renderCell} plain={plain} /></td>)}
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      <div className="data-table-foot">
        <Text size="caption" tone="secondary" tabular>{t('conversa.cartoes.tabela.tamanho', { linhas: t.plural('conversa.cartoes.tabela.linhasDe', rows.length, { visiveis: formatarNumero(idioma, shown.length) }), colunas: t.plural('conversa.cartoes.tabela.colunas', columns) })}</Text>
        {status ? <Text size="caption" tone="secondary" role="status" truncate>{status}</Text> : null}
        {short ? (
          <span className="data-table-foot-tools">
            <button className="data-table-tool" title={t('conversa.cartoes.tabela.copiar')} aria-label={t('conversa.cartoes.tabela.copiarAria')} onClick={copy}>
              <Icon name="doc.on.doc" size={15} />
            </button>
            {onExpand ? (
              <button className="data-table-tool" title={t('conversa.cartoes.tabela.expandirDica')} aria-label={t('conversa.cartoes.tabela.expandirAria')} onClick={onExpand}>
                <Icon name="arrow.up.left.and.arrow.down.right" size={14} />
              </button>
            ) : null}
          </span>
        ) : null}
      </div>
    </div>
  )
}
