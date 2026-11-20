import { useEffect, useRef, useState } from 'react'
import type { Engine, ManagedAccount } from '../../../../shared/fork-engine-accounts.ts'
import { Button, CardForm, CardSection, ChoiceList } from '../../../components/Controls'
import { ModalHeader, SheetPage } from '../../../components/Overlay'
import { HStack, Spacer, VStack } from '../../../components/Stack'
import { Text } from '../../../components/Text'
import { useModel } from '../../../lib/observable'
import { useStore } from '../../../store/context'
import { Field } from '../../marketplace/pieces'
import type { ContasStore } from './contas-store'
import { ENGINES, engineCompany, engineName, errorMessage, newRequestId, validLabel, type AccountKind } from './regras'
import { useTextosContas } from './use-textos'
import { superficieIdioma, useIdioma } from '../../../lib/fork-i18n/estado'
import { traduzir } from '../../../lib/fork-i18n/index.ts'
import './contas-ia.css'

/** Criar uma conta (Codex ou Claude Code; login ou chave) com um nome; depois segue para conectar. */
export function NovaConta({ contas, initialEngine, onCreated, onClose }: {
  contas: ContasStore
  initialEngine?: Engine
  onCreated: (account: ManagedAccount, kind: AccountKind) => void
  onClose: () => void
}) {
  const store = useStore()
  useModel(contas)
  const [engine, setEngine] = useState<Engine>(initialEngine ?? 'codex')
  const [kind, setKind] = useState<AccountKind>('cli')
  const [label, setLabel] = useState('')
  const [error, setError] = useState<unknown>(null)
  const textos = useTextosContas()
  const idioma = useIdioma()
  // Clique duplo e nova tentativa com os mesmos dados usam o mesmo pedido; dados novos, pedido novo.
  const requestId = useRef(newRequestId())
  useEffect(() => { requestId.current = newRequestId() }, [engine, kind, label])
  const creating = contas.busy === 'create'
  const valid = validLabel(label)

  const create = async () => {
    if (!valid || contas.busy) return
    setError(null)
    try {
      const account = await contas.create({ backend: engine, label: label.trim(), kind, clientRequestId: requestId.current })
      onCreated(account, kind)
    } catch (e) {
      setError(e)
    }
  }

  return (
    <SheetPage><div className="idioma-superficie" style={{ display: 'contents' }} {...superficieIdioma(idioma)}>
      <ModalHeader icon="person.crop.circle.badge.plus" title={textos.novaContaTitulo} subtitle={textos.novaContaSub(store.hostName)} closeTitle={textos.fechar} onClose={onClose} />
      <div className="contas-panel-body">
        <CardForm>
          <CardSection title={textos.qualMotor}>
            <ChoiceList
              selection={engine}
              options={ENGINES.map((e) => ({ id: e, label: engineName(e), detail: engineCompany(e) }))}
              onChange={(next) => { setEngine(next); if (next === 'grok') setKind('cli') }}
            />
          </CardSection>
          <CardSection title={textos.comoEntra}>
            <ChoiceList
              selection={kind}
              options={[
                { id: 'cli', label: textos.porLogin, detail: textos.porLoginDetalhe },
                ...(store.isLoopback && engine !== 'grok' ? [{ id: 'api' as const, label: textos.porChave, detail: textos.porChaveDetalhe }] : []),
              ]}
              onChange={setKind}
            />
          </CardSection>
          <CardSection title={textos.rotulo} footer={textos.rotuloDica}>
            <form onSubmit={(e) => { e.preventDefault(); void create() }}>
              <Field value={label} onChange={setLabel} label={textos.rotulo} placeholder={traduzir(idioma, 'contas.rotuloExemplo')} />
            </form>
          </CardSection>
          {error ? <Text tone="danger" role="alert" className="selectable">{errorMessage(error, idioma)}</Text> : null}
        </CardForm>
      </div>
      <HStack gap={8} className="contas-footer">
        <VStack shrink>
          <Text size="caption" tone="secondary">{kind === 'api' ? textos.chaveCusto : textos.introSemCusto}</Text>
        </VStack>
        <Spacer />
        <Button kind="secondary" onClick={onClose}>{textos.cancelar}</Button>
        <Button disabled={!valid || creating || contas.busy !== null} onClick={() => void create()}>{creating ? textos.criando : textos.criarEConectar}</Button>
      </HStack>
    </div></SheetPage>
  )
}
