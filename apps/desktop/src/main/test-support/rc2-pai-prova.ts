// Pai da prova de morte do processo principal: roda como processo Node separado, monta o
// artefato com o descritor aberto do binário copiado (na mesma forma que a verificação do
// artefato devolve), sobe o ciclo de vida real, confirma o hello e fica vivo para sempre —
// quem mata este processo é o teste observador, sem stop nem will-quit. Fixture de teste.
import { openSync, writeSync } from 'node:fs'
import { Rc2HubLifecycle, type Rc2Deadlines, type Rc2Schedule } from '../fork-rc2-local-hub.ts'
import { rc2NativeTransport } from '../fork-rc2-transport.ts'

const deadlines: Rc2Deadlines = { open: 3000, hello: 400, close: 1000, eof: 300, sigterm: 400, sigkill: 400, probe: 20 }
const schedule: Rc2Schedule = (ms, fire) => {
  const timer = setTimeout(fire, ms)
  return () => clearTimeout(timer)
}

const caminho = process.argv[2]
if (!caminho) process.exit(2)

const sessao = {}
const transporte = rc2NativeTransport({ components: [{ role: 'hub', path: caminho, fd: openSync(caminho, 'r') }], close: () => { } })
if (!transporte) process.exit(3)

const ciclo = new Rc2HubLifecycle(() => sessao, transporte, { deadlines, schedule })
const aberto = await ciclo.start()
const hello = aberto ? await ciclo.hello() : null
if (!hello) process.exit(4)
writeSync(1, 'pronto\n')
setInterval(() => { }, 60000)
