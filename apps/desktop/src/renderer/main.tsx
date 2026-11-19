// MX-93: entrada mínima. A abertura sorteada do index.html só pinta se o JS pesado do app não ocupar a thread
// antes do primeiro quadro (sem isso, a primeira pintura vinha depois do bundle inteiro). Na janela principal com
// abertura, espera um quadro pintado (ou 100 ms, se a janela não pintar) e carrega o app; nas outras, carrega já.
import { errorReport } from '@shared/error-report'

const ESPERA_MAXIMA_MS = 100

function depoisDoPrimeiroQuadro() {
  return new Promise<void>((pronto) => {
    requestAnimationFrame(() => setTimeout(pronto, 0))
    setTimeout(pronto, ESPERA_MAXIMA_MS)
  })
}

// MX-209: a janela do cartão seguro não tem `window.codync` (preload estreito próprio): entrada à parte, sem o app.
const cofreSeguro = new URLSearchParams(location.search).get('window') === 'cofre-seguro'
const temAbertura = !cofreSeguro && /^(pingo|sesta)$/.test(document.documentElement.dataset.mxAbertura ?? '')
void (temAbertura ? depoisDoPrimeiroQuadro() : Promise.resolve())
  .then(() => (cofreSeguro ? import('./views/fork/cofre-seguro/entrada').then(({ abrir }) => abrir()) : import('./inicio').then(() => undefined)))
  .catch((error: unknown) => {
    // Sem o app, a abertura não pode ficar por cima do erro para sempre.
    document.getElementById('mx-abertura')?.remove()
    window.codync?.app.reportError(errorReport(error, 'desktop-renderer', 'window-import'))
  })
