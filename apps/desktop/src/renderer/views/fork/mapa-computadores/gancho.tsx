// Gancho estreito em Ajustes → Computadores. Com a constante em `false`, o `import()` nem existe no
// bundle: o ternário é dobrado na compilação, e o módulo da demonstração e as fixtures não são emitidos.
import { lazy, Suspense } from 'react'

export const MAPA_COMPUTADORES_DEMO = false

const MapaDemo = MAPA_COMPUTADORES_DEMO
  ? lazy(() => import('./MapaComputadoresDemo.tsx').then((m) => ({ default: m.MapaComputadoresDemo })))
  : null

export function GanchoMapaComputadores() {
  if (!MapaDemo) return null
  return (
    <Suspense fallback={null}>
      <MapaDemo />
    </Suspense>
  )
}
