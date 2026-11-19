import { useEffect, useRef, useState, type KeyboardEvent } from 'react'
import { APP_ICON_CHOICES, APP_ICON_DEFAULT_ID, type AppIconId } from '@shared/app-icons'
import { superficieIdioma, useIdioma, useT } from '../../../lib/fork-i18n/estado'
import { radioTabStop } from '../../../lib/radio-group'
import { Icon } from '../../../components/Icon'
import { Text } from '../../../components/Text'
import './icone-app.css'

const COLUMNS = 4
const thumbs = import.meta.glob<string>('../../../../../resources/app-icons/*.png', { eager: true, query: '?url', import: 'default' })

function thumbURL(file: string): string | undefined {
  for (const [path, url] of Object.entries(thumbs)) {
    if (path === file || path.endsWith(`/${file}`) || path.endsWith(`\\${file}`)) return url
  }
  return undefined
}

const rowOf = (index: number) => Math.floor(index / COLUMNS)
const colOf = (index: number) => index % COLUMNS
const lastColOfRow = (row: number, count: number) => Math.min(COLUMNS, count - row * COLUMNS) - 1
const indexAt = (row: number, col: number, count: number) => row * COLUMNS + Math.min(col, lastColOfRow(row, count))

/** MX-9: as opções prontas das rodadas 2 e 3, por manifesto. Só o `id` vai ao main; o PNG é resolvido lá. */
export function IconeApp() {
  const t = useT()
  const idioma = useIdioma()
  const [selected, setSelected] = useState<AppIconId>(APP_ICON_DEFAULT_ID)
  const rows = useRef<(HTMLButtonElement | null)[]>([])
  rows.current.length = APP_ICON_CHOICES.length
  const stop = radioTabStop(APP_ICON_CHOICES.map((choice) => choice.id), selected)

  useEffect(() => {
    let live = true
    void window.codync.appIcon.selected().then((id) => { if (live) setSelected(id) }).catch(() => {})
    return () => { live = false }
  }, [])

  const choose = (id: AppIconId) => {
    if (id === selected) return
    void window.codync.appIcon.set(id).then(setSelected).catch(() => {})
  }

  const onKeyDown = (event: KeyboardEvent<HTMLButtonElement>, index: number) => {
    if (event.altKey || event.ctrlKey || event.metaKey || event.shiftKey) return
    const count = APP_ICON_CHOICES.length
    const rowsCount = Math.ceil(count / COLUMNS)
    let next: number | null = null
    if (event.key === 'ArrowRight') next = (index + 1) % count
    else if (event.key === 'ArrowLeft') next = (index - 1 + count) % count
    else if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      const step = event.key === 'ArrowDown' ? 1 : -1
      const row = (rowOf(index) + step + rowsCount) % rowsCount
      next = indexAt(row, Math.min(colOf(index), lastColOfRow(row, count)), count)
    }
    if (next === null) return
    event.preventDefault()
    event.stopPropagation()
    rows.current[next]?.focus()
    const id = APP_ICON_CHOICES[next]!.id
    if (id !== selected) choose(id)
  }

  return (
    <div className="icone-app idioma-superficie" {...superficieIdioma(idioma)}>
      <Text as="div" size="callout" tone="secondary" className="icone-app-descricao">{t('aparencia.icone.descricao')}</Text>
      <div className="icone-app-grade" role="radiogroup" aria-label={t('aparencia.icone.opcoes.aria')}>
        {APP_ICON_CHOICES.map((choice, index) => {
          const checked = choice.id === selected
          const nome = t(choice.nameKey)
          return (
            <button
              key={choice.id}
              ref={(el) => { rows.current[index] = el }}
              type="button"
              className="icone-app-opcao"
              role="radio"
              aria-checked={checked}
              aria-label={nome}
              title={nome}
              tabIndex={index === stop ? 0 : -1}
              onClick={() => choose(choice.id)}
              onKeyDown={(event) => onKeyDown(event, index)}
            >
              <span className="icone-app-quadriculado">
                <img src={thumbURL(choice.file)} alt="" width={56} height={56} draggable={false} />
              </span>
              <span className="icone-app-nome">{nome}</span>
              {checked ? <span className="icone-app-check" aria-hidden><Icon name="checkmark" size={10} weight="semibold" /></span> : null}
            </button>
          )
        })}
      </div>
      <Text as="div" size="footnote" tone="tertiary" className="icone-app-nota">{t('aparencia.icone.aviso')}</Text>
    </div>
  )
}
