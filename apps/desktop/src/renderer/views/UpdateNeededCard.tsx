// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import { Button } from '../components/Controls'
import { avisoDoStore } from '../lib/fork-aviso-desatualizado.ts'
import { useT } from '../lib/fork-i18n/estado'
import { useApp, useStore } from '../store/context'
import { AvisoComputador, useFaseAviso } from './AvisoComputador'
import { UpdateButton, useUpdates } from './UpdateButton'
import { comNos, no } from './settings/rotulo'

/**
 * O aviso de versão de UM computador (ou deste app), no lugar do compositor e acima dos bots daquele computador.
 * Este conector junta o estado do store, a atualização do app e o botão; o visual é `AvisoComputador`.
 */
export function UpdateNeededCard() {
  const app = useApp()
  const store = useStore()
  const updates = useUpdates()
  const t = useT()
  const aviso = avisoDoStore(store)
  const fase = useFaseAviso(aviso, store.connection.kind === 'online')
  if (fase === 'oculto') return null
  // Lição das R1 do #570/#577: o nome do computador (dado do usuário) fica isolado dentro da frase.
  const host = <bdi dir="auto">{store.hostName}</bdi>
  // O app se atualiza sozinho (download e reinício, como em Ajustes); no macOS e no Windows o computador deste
  // app volta para a cópia que vem com ele. O Linux não traz essa cópia: o aviso diz como atualizar.
  const doApp = aviso?.alvo === 'app'
  const reiniciaLocal = !doApp && store === app.local && !app.host.dev && window.codync.platform !== 'linux'
  const os = store.hello?.os
  const como = doApp ? t('navegacao.janela.desatualizado.fazerApp')
    : reiniciaLocal ? t('navegacao.janela.desatualizado.fazerLocal')
    // O nome do binário é técnico: entra como parâmetro, fora do catálogo (D443).
    : os === 'linux' ? t('navegacao.janela.desatualizado.fazerLinux', { binario: 'donnabot-host' }) // textos-ok: nome técnico do binário
    : os === 'macos' ? t('marca.atualizar.mac')
    : t('marca.atualizar.outro')
  const acao = doApp
    ? updates && <UpdateButton updates={updates} kind="primary" />
    : reiniciaLocal && <Button onClick={() => window.codync.host.restart()}>{comNos(t('navegacao.janela.desatualizado.botaoLocal', { host: no('host') }), { host })}</Button>
  return <AvisoComputador t={t} fase={fase} host={host} aviso={aviso} como={como} acao={acao || undefined} />
}
