import { conjuntoPorId } from '../../lib/mexeriquinhas/conjuntos.ts'
import { QUADROS } from '../../lib/mexeriquinhas/render.ts'
import type { QuadroPelucia } from '../../lib/mexeriquinhas-runtime.ts'

// Vite emite arquivos separados. Eager importa só as URLs: não baixa/decodifica os WebP.
// no-inline impede base64 até nos quadros pequenos; nenhuma imagem no bundle JavaScript.
const urls: Record<string, string> = (() => {
  try { return import.meta.glob<string>('../../lib/mexeriquinhas/assets/*/*/*.webp', { eager: true, query: '?no-inline&url', import: 'default' }) }
  catch { return {} } // Testes Node sem o transform do Vite.
})()

// Só o menu nativo precisa de data URL para o canvas file://. Chunks sob demanda:
// importar o mapa não incorpora os pixels no renderer, e só o quadro usado é lido.
// A bandeja pede sempre o primeiro quadro do estado (native-mexeriquinha.ts):
// os 5 abaixo bastam — os quadros de animação nunca viram ícone.
const inline: Record<string, () => Promise<string>> = (() => {
  try {
    // import.meta.glob exige padrão literal — um por quadro, não um laço.
    return {
      ...import.meta.glob<string>('../../lib/mexeriquinhas/assets/*/64/livre-1.webp', { query: '?inline', import: 'default' }),
      ...import.meta.glob<string>('../../lib/mexeriquinhas/assets/*/64/trabalhando-1.webp', { query: '?inline', import: 'default' }),
      ...import.meta.glob<string>('../../lib/mexeriquinhas/assets/*/64/dormindo-1.webp', { query: '?inline', import: 'default' }),
      ...import.meta.glob<string>('../../lib/mexeriquinhas/assets/*/64/precisa-resposta.webp', { query: '?inline', import: 'default' }),
      ...import.meta.glob<string>('../../lib/mexeriquinhas/assets/*/64/erro.webp', { query: '?inline', import: 'default' }),
    }
  }
  catch { return {} }
})()

// Conjuntos gerados pelo Hub (v3): uuid v4 → transporte autenticado mxq:// no main.
// ID conhecido não é autorização; o Hub valida o conjunto registrado a cada pedido.
export const assetIdGerado = (id: string): boolean =>
  /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/.test(id)

export const urlDoQuadroGerado = (assetId: string, size: number, quadro: QuadroPelucia): string =>
  `mxq://conjunto/${assetId}/${size <= 40 ? 64 : 256}/${quadro}.webp`

export async function quadroPeluciaParaCanvas(conjuntoId: string, quadro: QuadroPelucia): Promise<string | undefined> {
  if (!QUADROS.includes(quadro)) return undefined
  if (assetIdGerado(conjuntoId)) return urlDoQuadroGerado(conjuntoId, 64, quadro)
  if (!conjuntoPorId(conjuntoId)) return undefined
  return inline[`../../lib/mexeriquinhas/assets/${conjuntoId}/64/${quadro}.webp`]?.()
}

export function srcDoQuadroPelucia(conjuntoId: string, size: number, quadro: QuadroPelucia): string | undefined {
  if (!QUADROS.includes(quadro)) return undefined
  if (assetIdGerado(conjuntoId)) return urlDoQuadroGerado(conjuntoId, size, quadro)
  if (!conjuntoPorId(conjuntoId)) return undefined
  return urls[`../../lib/mexeriquinhas/assets/${conjuntoId}/${size <= 32 ? 64 : size <= 64 ? 128 : 256}/${quadro}.webp`]
}
