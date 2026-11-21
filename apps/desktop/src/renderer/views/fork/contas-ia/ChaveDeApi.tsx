import { useState, type FormEvent } from 'react'
import type { Engine, ManagedAccount } from '../../../../shared/fork-engine-accounts.ts'
import { Button, CardForm, CardSection } from '../../../components/Controls'
import { ModalHeader, SheetPage } from '../../../components/Overlay'
import { HStack, Spacer, VStack } from '../../../components/Stack'
import { Text } from '../../../components/Text'
import { useModel } from '../../../lib/observable'
import { useStore } from '../../../store/context'
import { AgentTile } from '../../marketplace/AgentSheet'
import { Field, WebLink } from '../../marketplace/pieces'
import type { ContasStore } from './contas-store'
import { engineCompany, engineRegistry, errorMessage, keyPageUrl } from './regras'
import { useTextosContas } from './use-textos'
import { superficieIdioma, useIdioma } from '../../../lib/fork-i18n/estado'
import { traduzir } from '../../../lib/fork-i18n/index.ts'
import './contas-ia.css'

/** A única variável que o Hub aceita para a chave de cada provedor, e a página oficial onde ela é criada. */
const API_KEY: Record<Exclude<Engine, 'grok'>, { name: string; rotulo: 'contas.chaveRotulo.codex' | 'contas.chaveRotulo.claude'; page: string }> = {
  codex: { name: 'OPENAI_API_KEY', rotulo: 'contas.chaveRotulo.codex', page: 'https://platform.openai.com/api-keys' },
  claude: { name: 'ANTHROPIC_API_KEY', rotulo: 'contas.chaveRotulo.claude', page: 'https://console.anthropic.com/settings/keys' },
}

/**
 * A chave de API de uma conta, gravada no cofre do Hub só para ela. Sem chamada de teste paga
 * e sem virar reserva de outra conta; o valor some da tela ao salvar ou fechar.
 */
export function ChaveDeApi({ contas, account, onClose }: { contas: ContasStore; account: ManagedAccount; onClose: () => void }) {
  const store = useStore()
  useModel(contas)
  // O Grok entra só pelo login do programa oficial: o Hub recusa chave e a tela nunca abre este formulário para ele.
  const key = API_KEY[account.backend as Exclude<Engine, 'grok'>]
  // Segredo: não sobrevive à troca de página, de propósito (estado-pagina.ts, exceções).
  const [value, setValue] = useState('')
  const [error, setError] = useState<unknown>(null)
  const textos = useTextosContas()
  const idioma = useIdioma()
  const saving = contas.busy === account.id
  const link = keyPageUrl(account.backend, key.page)
  const replacing = account.billing === 'paid_api' && account.state === 'connected'

  const save = async (e?: FormEvent) => {
    e?.preventDefault()
    if (!value.trim() || contas.busy) return
    setError(null)
    try {
      await contas.saveKey(account, { [key.name]: value.trim() })
      setValue('')
      onClose()
    } catch (err) {
      setError(err)
    }
  }

  return (
    <SheetPage><div className="idioma-superficie" style={{ display: 'contents' }} {...superficieIdioma(idioma)}>
      <ModalHeader
        icon={<AgentTile registry={engineRegistry(account.backend)} icon={18} tile={32} radius={9} />}
        title={textos.chaveTitulo(account.label)}
        closeTitle={textos.fechar}
        subtitle={textos.chaveSub(engineCompany(account.backend), store.hostName)}
        onClose={onClose}
      />
      <div className="contas-panel-body">
        <CardForm>
          <CardSection title={traduzir(idioma, key.rotulo)} footer={replacing ? textos.chaveSalva : null}>
            <form onSubmit={(e) => void save(e)}>
              <Field label={traduzir(idioma, key.rotulo)} value={value} secret placeholder={key.name} onChange={setValue} />
            </form>
          </CardSection>
          <VStack gap={6} className="agent-keys-notes">
            <span>{textos.chaveNotas}</span>
            <span>{textos.chaveCusto}</span>
            {link ? <WebLink title={textos.pegarChave} url={link} /> : null}
          </VStack>
          {error ? <Text tone="danger" role="alert" className="selectable">{errorMessage(error, idioma)}</Text> : null}
        </CardForm>
      </div>
      <HStack gap={8} className="contas-footer">
        <Spacer />
        <Button kind="secondary" onClick={() => { setValue(''); onClose() }}>{textos.cancelar}</Button>
        <Button disabled={!value.trim() || saving || contas.busy !== null} onClick={() => void save()}>{saving ? textos.salvando : textos.salvar}</Button>
      </HStack>
    </div></SheetPage>
  )
}
