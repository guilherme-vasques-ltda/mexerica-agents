// Entrada da web local: o Hub serve esta página na mesma origem (127.0.0.1) e a sessão é um
// cookie HttpOnly dele. A ponte (window.codync) é instalada antes de qualquer importe do
// renderer; sem sessão, o portão de acesso local pede o token do Hub. Se a sessão morrer com
// o app montado, a página recarrega: o estado do app nunca fica para trás (fail-closed).
import { errorReport } from '@shared/error-report'
import { FALLBACK_LANGUAGE, sugerirIdioma } from '../shared/fork-idioma.ts'
import { superficieIdioma, tradutor } from '../renderer/lib/fork-i18n/index.ts'
import { instalarIdiomaWeb } from './idioma.ts'
import { createWebBridge } from './platform.ts'
import { createLocalWebHost, LocalWebSession } from './local-hub.ts'
import { createLocalFiles } from './local-files.ts'
import { montarPortaoLocal } from './local-gate.tsx'
import './shell/shell.css'

/** Marcador não secreto de que a visita anterior perdeu a sessão (o token nunca vem para cá). */
const MARCADOR_EXPIRADA = 'mx-web-sessao-expirada'

// Tema salvo antes do primeiro quadro (a CSP não permite script inline). O renderer confirma
// com startAppearance; aqui é só para a primeira pintura já sair no tema da pessoa.
try {
  const tema = JSON.parse(localStorage.getItem('appearanceTheme') ?? 'null') as unknown
  if (tema === 'light' || tema === 'dark') document.documentElement.dataset.theme = tema
} catch { /* sem tema salvo ou sem armazenamento: segue o tema do navegador */ }

const origin = window.location.origin
const session = new LocalWebSession(origin)

// A visita anterior terminou com a sessão expirada: o portão avisa em vez de abrir mudo.
// O marcador é um booleano; CSRF e metadados da sessão vivem só na memória.
let expirada = false
try {
  if (sessionStorage.getItem(MARCADOR_EXPIRADA) === '1') {
    sessionStorage.removeItem(MARCADOR_EXPIRADA)
    expirada = true
  }
} catch { /* sem armazenamento: sem aviso, sem erro */ }

// Sessão que expirou com o app montado volta limpa ao portão: recarrega, sem estado do app.
void session.onExpired(() => {
  try { sessionStorage.setItem(MARCADOR_EXPIRADA, '1') } catch { /* sem aviso, sem erro */ }
  location.reload()
})

const autenticado = await session.checkSession()
const porta = Number(new URL(origin).port)
// A ponte nasce com o nome do computador que a sessão tem no momento; depois do login (portão)
// ela é reinstalada para o renderer receber o `hostName` real do Hub, não o fallback.
const instalarPonte = () => {
  window.codync = createWebBridge({
    host: createLocalWebHost(session),
    files: createLocalFiles(),
    hostPort: porta > 0 && porta < 65536 ? porta : 19422,
    computerName: session.hostName ?? 'Navegador',
    appScheme: 'donnabot',
  })
  instalarIdiomaWeb(window.codync)
}
instalarPonte()
// Sem o forkCloud real do navegador não há estado de link: a faixa de queda nunca afirma queda.
document.documentElement.dataset.webQueda = 'inerte'

// A importação do renderer pode falhar depois do login (bundle corrompido, rede caída no
// meio): o #root nunca fica mudo. Textos do mesmo catálogo do portão, DOM seguro (sem
// innerHTML, sem estilo inline) — a CSP não abre exceção para a recuperação.
const montarRecuperacaoImporte = () => {
  const idioma = sugerirIdioma(navigator.languages) ?? FALLBACK_LANGUAGE
  const t = tradutor(idioma)
  const superficie = superficieIdioma(idioma)
  const raiz = document.getElementById('root')
  if (raiz) raiz.textContent = ''
  const pagina = document.createElement('div')
  pagina.className = 'login-web'
  pagina.lang = superficie.lang
  pagina.dir = superficie.dir
  const coluna = document.createElement('main')
  coluna.className = 'login-web-coluna'
  const titulo = document.createElement('h1')
  titulo.className = 'login-web-titulo'
  titulo.textContent = t('web.login.local.titulo')
  const texto = document.createElement('p')
  texto.className = 'login-web-texto'
  texto.textContent = t('web.login.local.erro.geral')
  const botao = document.createElement('button')
  botao.type = 'button'
  botao.className = 'login-web-primario'
  botao.textContent = t('web.login.local.entrar')
  botao.addEventListener('click', () => window.location.reload())
  coluna.append(titulo, texto, botao)
  pagina.append(coluna)
  ;(raiz ?? document.body).append(pagina)
}

const abrirApp = () => import('../renderer/main.tsx').catch((error: unknown) => {
  window.codync.app.reportError(errorReport(error, 'desktop-renderer', 'window-import'))
  montarRecuperacaoImporte()
})

if (autenticado) {
  void abrirApp()
} else {
  const desmontar = montarPortaoLocal(document.getElementById('root')!, session, expirada, () => {
    desmontar()
    // O login acabou de completar: a ponte sai do fallback 'Navegador' e ganha o hostName real.
    instalarPonte()
    void abrirApp()
  })
}
