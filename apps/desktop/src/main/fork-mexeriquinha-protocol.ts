// Fase 2b: quadros de conjuntos 3D gerados pelo Hub chegam por `mxq://conjunto/<assetId>/<tamanho>/<quadro>.webp`.
// O esquema é servido só pelo main, que autentica no Hub; o renderer nunca vê token nem segredo.
// Um assetId conhecido não é autorização — o Hub confere o conjunto registrado a cada pedido.
import { protocol } from 'electron'
import type { HostController } from './host-controller'

export const SCHEME = 'mxq'
const QUADROS = new Set(['livre-1', 'livre-2', 'piscar', 'trabalhando-1', 'trabalhando-2', 'precisa-resposta', 'erro', 'dormindo-1', 'dormindo-2'])
const ASSET_ID = /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/

/** Privilégios antes do app ready; esquema próprio, sem CORS nem acesso de fora. */
export function registerScheme() {
  // O harness de testes do main roda sem Electron completo; sem protocol não há esquema.
  if (typeof protocol?.registerSchemesAsPrivileged !== 'function') return
  protocol.registerSchemesAsPrivileged([{ scheme: SCHEME, privileges: { standard: false, secure: true, supportFetchAPI: true, stream: true } }])
}

export function registerProtocol(host: HostController) {
  if (typeof protocol?.handle !== 'function') return
  protocol.handle(SCHEME, async (request) => {
    const fail = (status = 404) => new Response(null, { status })
    let url: URL
    try {
      url = new URL(request.url)
    } catch {
      return fail()
    }
    if (url.hostname !== 'conjunto') return fail()
    const partes = url.pathname.split('/').filter(Boolean)
    if (partes.length !== 3) return fail()
    const [assetId, tamanhoRaw, arquivo] = partes
    const quadro = arquivo!.replace(/\.webp$/, '')
    const tamanho = Number(tamanhoRaw)
    if (!ASSET_ID.test(assetId!) || ![64, 256].includes(tamanho) || !QUADROS.has(quadro) || arquivo !== `${quadro}.webp`) return fail()
    if (!host.local) return fail(503)
    try {
      const resposta = await host.call<{ data?: string }>('mexeriquinhaQuadro', { assetId, tamanho, quadro }, 10_000)
      if (typeof resposta?.data !== 'string') return fail()
      const bytes = new Uint8Array(Buffer.from(resposta.data, 'base64'))
      return new Response(bytes, { headers: { 'Content-Type': 'image/webp', 'Cache-Control': 'private, max-age=86400' } })
    } catch {
      return fail()
    }
  })
}
