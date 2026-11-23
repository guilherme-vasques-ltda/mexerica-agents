import { useEffect, useRef, useState, type CSSProperties, type KeyboardEvent, type ReactNode } from 'react'
import { direcaoDe, type AppLanguage } from '@shared/fork-idioma'
import { appearancePrefs, useResolvedDark } from '../../../lib/appearance'
import { idiomaPref, superficieIdioma, tradutor, useIdioma } from '../../../lib/fork-i18n/estado'
import { usePref } from '../../../lib/prefs'
// D471: a mexerica do pacote oficial (brand/assets/logo/04-mascote, cópia byte a byte), no lugar do "d".
import marca from '../../../../../resources/brand/mexerica/mexerica-mascote-256.png'
import { marcarPassoIdioma, ORDEM_RODA, passoIdiomaPendente, teclaRoda } from './passo'
import { AvisosIdioma } from './AvisosIdioma'
import './entrada-marca.css'
import './passo-idioma.css'
import './escritas.css'

function armazenamento(): Storage | null {
  try {
    return globalThis.localStorage ?? null
  } catch {
    return null
  }
}

/**
 * Se o passo 1 aparece antes da conta; `concluir` o encerra neste computador (Continuar ou Enter).
 * `concluido` só nesta abertura, para a entrada receber o foco ao chegar dele.
 */
export function usePassoIdioma() {
  const [pendente, setPendente] = useState(() => passoIdiomaPendente(armazenamento()))
  const [concluido, setConcluido] = useState(false)
  return { pendente, concluido, concluir: () => { marcarPassoIdioma(armazenamento()); setPendente(false); setConcluido(true) } }
}

/** Passos da entrada e configuracao inicial: idioma, conta, lugar/IA e primeira conversa. */
const TOTAL_PASSOS = 4

/**
 * DON-903, passo 1 da entrada (proposta aprovada da #426): roda de idiomas com o sugerido pelo sistema já
 * escolhido. Cada movimento grava o idioma e a tela inteira muda na hora. Só existe dentro das boas-vindas,
 * com a conta ainda não confirmada: não monta nada do app nem do Hub e não decide o portão.
 */
export function PassoIdioma({ sugerido, onContinuar }: { sugerido: AppLanguage | null; onContinuar: () => void }) {
  const idioma = useIdioma()
  const t = tradutor(idioma)
  const roda = useRef<HTMLDivElement>(null)
  // A primeira abertura começa no sugerido; sem sugestão, em pt-BR (o fallback já em uso).
  useEffect(() => {
    if (idiomaPref.get() === null && sugerido) idiomaPref.set(sugerido)
    roda.current?.focus({ preventScroll: true })
  }, [sugerido])

  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    const acao = teclaRoda(e, idioma)
    if (!acao) return
    e.preventDefault()
    if ('continuar' in acao) onContinuar()
    else idiomaPref.set(acao.escolher)
  }
  const i = Math.max(0, ORDEM_RODA.indexOf(idioma))

  return (
    <div className="passo-idioma idioma-superficie" {...superficieIdioma(idioma)}>
      <header className="passo-topo">
        <span />
        <div className="passo-etapas" role="img" aria-label={t('idioma.passo.etapa', { atual: 1, total: TOTAL_PASSOS })}>
          {Array.from({ length: TOTAL_PASSOS }, (_, n) => <span key={n} className={n === 0 ? 'agora' : undefined} />)}
        </div>
        <EscolhaTema t={t} />
      </header>
      <main className="passo-centro">
        <img className="passo-marca" src={marca} alt="" />
        <h1 className="passo-titulo">{t('idioma.passo.titulo1')}<br />{t('idioma.passo.titulo2')}</h1>
        <p className="passo-sub">{t('idioma.passo.subtitulo')}</p>
        <p className="passo-rotulo" id="passo-idioma-rotulo">{t('idioma.passo.escolha')}</p>
        <div
          ref={roda}
          className="roda"
          role="listbox"
          tabIndex={0}
          aria-labelledby="passo-idioma-rotulo"
          aria-activedescendant={`roda-${idioma}`}
          onKeyDown={onKeyDown}
          style={{ '--centro': i } as CSSProperties}
        >
          <div className="roda-faixa" aria-hidden />
          <ul>
            {ORDEM_RODA.map((id, j) => (
              <li
                key={id}
                id={`roda-${id}`}
                role="option"
                lang={id}
                dir={direcaoDe(id)}
                aria-selected={id === idioma}
                className={j === i ? undefined : `d${Math.min(Math.abs(j - i), 3)}`}
                onClick={() => { idiomaPref.set(id); roda.current?.focus() }}
              >
                <Bandeira idioma={id} />
                {tradutor(id)('idioma.nome')}
              </li>
            ))}
          </ul>
        </div>
        {idioma === sugerido ? (
          <p className="passo-dica">
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" strokeLinejoin="round" aria-hidden><path d="M20 6 9 17l-5-5" /></svg>
            {t('idioma.passo.sugerido')}
          </p>
        ) : <p className="passo-dica" aria-hidden>&nbsp;</p>}
        <AvisosIdioma idioma={idioma} className="passo-avisos" />
        <button type="button" className="passo-continuar" onClick={onContinuar}>
          {t('idioma.passo.continuar')}
          <svg className="espelhar-rtl" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" strokeLinejoin="round" aria-hidden><path d="M5 12h14M13 6l6 6-6 6" /></svg>
        </button>
      </main>
      <footer className="passo-rodape">{t('idioma.passo.rodape')}</footer>
    </div>
  )
}

/** Claro/Escuro no topo, gravando a mesma preferência de Ajustes → Aparência (Sistema segue valendo até a escolha). */
function EscolhaTema({ t }: { t: ReturnType<typeof tradutor> }) {
  const [, setTema] = usePref(appearancePrefs.theme)
  const escuro = useResolvedDark()
  const opcoes = [
    { id: 'light' as const, ativo: !escuro, rotulo: t('tema.claro'), icone: <><circle cx="12" cy="12" r="4" /><path d="M12 2v2M12 20v2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M2 12h2M20 12h2M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4" /></> },
    { id: 'dark' as const, ativo: escuro, rotulo: t('tema.escuro'), icone: <path d="M12 3a6 6 0 0 0 9 9 9 9 0 1 1-9-9Z" /> },
  ]
  return (
    <div className="passo-tema" role="radiogroup" aria-label={t('tema.grupo')}>
      {opcoes.map((o) => (
        <button key={o.id} type="button" role="radio" aria-checked={o.ativo} aria-label={o.rotulo} onClick={() => setTema(o.id)}>
          <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden>{o.icone}</svg>
        </button>
      ))}
    </div>
  )
}

const BANDEIRAS: Record<AppLanguage, ReactNode> = {
  'pt-BR': <><rect width="22" height="15" fill="#1f8f4e" /><path d="M11 2 20 7.5 11 13 2 7.5Z" fill="#f4c61f" /><circle cx="11" cy="7.5" r="3.3" fill="#20418f" /></>,
  en: <><rect width="22" height="15" fill="#fff" /><g fill="#c33">{[0, 2, 4, 6, 8, 10, 12].map((n) => <rect key={n} y={(n * 15) / 13} width="22" height={15 / 13} />)}</g><rect width="10" height="8" fill="#2b4a8b" /></>,
  es: <><rect width="22" height="15" fill="#c8102e" /><rect y="3.75" width="22" height="7.5" fill="#f6c400" /></>,
  fr: <><rect width="7.34" height="15" fill="#21468b" /><rect x="7.33" width="7.34" height="15" fill="#fff" /><rect x="14.66" width="7.34" height="15" fill="#d52b1e" /></>,
  // Simplificadas, como as da prévia: China (estrela) e Emirados Árabes Unidos (faixas e barra).
  'zh-CN': <><rect width="22" height="15" fill="#de2910" /><path d="M4.5 2.2l.9 2.7h2.8l-2.3 1.7.9 2.7-2.3-1.7-2.3 1.7.9-2.7L.8 4.9h2.8z" fill="#ffde00" /></>,
  'ar-AE': <><rect width="22" height="15" fill="#fff" /><rect width="22" height="5" fill="#00732f" /><rect y="10" width="22" height="5" fill="#000" /><rect width="6" height="15" fill="#ff0000" /></>,
}

/** Bandeiras simplificadas da prévia aprovada (sem emoji, sem arquivo externo), pela região do idioma. */
function Bandeira({ idioma }: { idioma: AppLanguage }) {
  return (
    <span className="roda-bandeira" aria-hidden>
      <svg viewBox="0 0 22 15">{BANDEIRAS[idioma]}</svg>
    </span>
  )
}
