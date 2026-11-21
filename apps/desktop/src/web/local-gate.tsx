// Portão de acesso local da web: antes do renderer, a página pede o token local que o Hub
// salvou neste computador. Layout do Login Web v5 (chão Creme, tintas marrons, Laranja da
// marca); todos os textos vêm do catálogo fork-i18n (chaves web.login.local.*). O token fica
// só no campo: nada vai para preferência, estado global ou URL — sai dele ao enviar e a
// sessão nasce como cookie HttpOnly do Hub, nunca visível ao JavaScript.
import { useState, type FormEvent } from 'react'
import { createRoot } from 'react-dom/client'
import type { AppLanguage } from '../shared/fork-idioma.ts'
import { FALLBACK_LANGUAGE, sugerirIdioma } from '../shared/fork-idioma.ts'
import { superficieIdioma, tradutor, type Tradutor } from '../renderer/lib/fork-i18n/index.ts'
import { idiomaPref } from '../renderer/lib/fork-i18n/estado.ts'
import { usePref } from '../renderer/lib/prefs.ts'
import type { ErroLogin, LocalWebSession } from './local-hub.ts'
import '../renderer/styles/tokens.css'
import '../renderer/styles/fonts.css'
import logo from '../../resources/brand/mexerica/mexerica-logo-claro.svg'
import logoEscuro from '../../resources/brand/mexerica/mexerica-logo-escuro.svg'
import mascote from '../../resources/brand/mexerica/mexerica-mascote-256.png'
import './local-gate.css'

/** A ordem do seletor de idioma: a mesma da entrada do desktop. */
const ORDEM_IDIOMAS: readonly AppLanguage[] = ['en', 'pt-BR', 'es', 'fr', 'zh-CN', 'ar-AE']

/** As chaves que este portão usa; nenhuma leva parâmetros. */
type ChavePortao =
  | 'web.login.local.tokenVazio'
  | 'web.login.local.erro.invalido'
  | 'web.login.local.erro.limite'
  | 'web.login.local.erro.offline'
  | 'web.login.local.erro.geral'
  | 'web.login.local.expirada'

/** A chave do catálogo para cada recusa; o texto do serviço nunca aparece. */
function chaveErro(erro: ErroLogin | 'vazio' | 'expirada'): ChavePortao {
  switch (erro) {
    case 'vazio': return 'web.login.local.tokenVazio'
    case 'invalid': return 'web.login.local.erro.invalido'
    case 'rateLimited': return 'web.login.local.erro.limite'
    case 'offline': return 'web.login.local.erro.offline'
    case 'error': return 'web.login.local.erro.geral'
    case 'expirada': return 'web.login.local.expirada'
  }
}

/** Os seis idiomas do app, cada nome no próprio idioma; a escolha vale para o app. */
function SeletorIdioma({ idioma, t }: { idioma: AppLanguage; t: Tradutor }) {
  return (
    <label className="login-web-idioma">
      <span className="login-web-oculto">{t('web.login.idioma')}</span>
      <select value={idioma} onChange={e => idiomaPref.set(e.target.value as AppLanguage)}>
        {ORDEM_IDIOMAS.map(i => (
          <option key={i} value={i} {...superficieIdioma(i)}>{tradutor(i)('idioma.nome')}</option>
        ))}
      </select>
    </label>
  )
}

export function PortaoAcessoLocal({ session, expirada = false, onAutenticado }: {
  session: LocalWebSession
  /** A sessão anterior morreu: o aviso diz antes de qualquer digitação. */
  expirada?: boolean
  onAutenticado: () => void
}) {
  const [escolhido] = usePref(idiomaPref)
  const idioma = escolhido ?? sugerirIdioma(typeof navigator === 'undefined' ? [] : navigator.languages) ?? FALLBACK_LANGUAGE
  const t = tradutor(idioma)
  const [token, setToken] = useState('')
  const [mostrar, setMostrar] = useState(false)
  const [ocupado, setOcupado] = useState(false)
  const [erro, setErro] = useState<ErroLogin | 'vazio' | 'expirada' | null>(expirada ? 'expirada' : null)

  const enviar = (e: FormEvent) => {
    e.preventDefault()
    if (ocupado) return
    if (!token.trim()) { setErro('vazio'); return }
    setErro(null)
    setOcupado(true)
    void session.login(token).then((categoria) => {
      setOcupado(false)
      if (categoria) { setErro(categoria); return }
      // O token sai daqui: o campo limpa e a sessão vive só no cookie HttpOnly do Hub.
      setToken('')
      onAutenticado()
    })
  }

  return (
    <div className="login-web" {...superficieIdioma(idioma)}>
      <header className="login-web-topo">
        <img className="login-web-logo so-claro" src={logo} alt="" draggable={false} />
        <img className="login-web-logo so-escuro" src={logoEscuro} alt="" draggable={false} />
        <SeletorIdioma idioma={idioma} t={t} />
      </header>
      <main className="login-web-coluna">
        <form className="login-web-formulario" onSubmit={enviar}>
          <p className="login-web-sobretitulo">{t('web.login.local.sobretitulo')}</p>
          <h1 className="login-web-titulo">{t('web.login.local.titulo')}</h1>
          <p className="login-web-texto">{t('web.login.local.subtitulo')}</p>
          <div className="login-web-campo">
            <label className="login-web-rotulo" htmlFor="token-local">{t('web.login.local.token')}</label>
            <div className="login-web-senha">
              <input
                id="token-local"
                className="login-web-entrada"
                type={mostrar ? 'text' : 'password'}
                value={token}
                onChange={e => { setToken(e.target.value); if (erro !== 'expirada') setErro(null) }}
                placeholder={t('web.login.local.tokenExemplo')}
                autoComplete="off"
                spellCheck={false}
                disabled={ocupado}
                aria-invalid={erro !== null}
                aria-describedby={erro ? 'erro-token' : undefined}
              />
              <button
                type="button"
                className="login-web-mostrar"
                aria-pressed={mostrar}
                onClick={() => setMostrar(!mostrar)}
                disabled={ocupado}
              >
                {mostrar ? t('web.login.ocultar') : t('web.login.mostrar')}
              </button>
            </div>
            {erro !== null && (
              <p className="login-web-erro-campo" id="erro-token" role="alert">{t(chaveErro(erro))}</p>
            )}
          </div>
          <button type="submit" className="login-web-primario" disabled={ocupado}>
            {ocupado ? t('web.login.local.entrando') : t('web.login.local.entrar')}
          </button>
          <p className="login-web-dica">{t('web.login.local.dica')}</p>
          <p className="login-web-endereco">
            {t('web.login.confiraEndereco')} <strong>127.0.0.1</strong>
          </p>
        </form>
      </main>
      <img className="login-web-mascote" src={mascote} alt="" draggable={false} />
      <div className="login-web-chao" />
    </div>
  )
}

/** Monta o portão no #root (main.ts é TypeScript puro, sem JSX); devolve a função de desmontar. */
export function montarPortaoLocal(raiz: HTMLElement, session: LocalWebSession, expirada: boolean, onAutenticado: () => void): () => void {
  const root = createRoot(raiz)
  root.render(<PortaoAcessoLocal session={session} expirada={expirada} onAutenticado={onAutenticado} />)
  return () => root.unmount()
}
