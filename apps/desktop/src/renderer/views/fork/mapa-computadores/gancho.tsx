// GUI-467: gancho estreito em Ajustes → Computadores. Desligado: sem a flag, nada daqui é importado nem renderizado,
// e a demonstração não tem caminho no build oficial nem no RC.
import { lazy, Suspense } from 'react'

export const MAPA_COMPUTADORES_DEMO = false

const MapaDemo = lazy(() => import('./MapaComputadoresDemo.tsx').then((m) => ({ default: m.MapaComputadoresDemo })))

export function GanchoMapaComputadores() {
  if (!MAPA_COMPUTADORES_DEMO) return null
  return (
    <Suspense fallback={null}>
      <MapaDemo />
    </Suspense>
  )
}
