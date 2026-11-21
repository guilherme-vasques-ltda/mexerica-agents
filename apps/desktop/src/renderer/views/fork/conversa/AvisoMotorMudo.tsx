// O aviso de que o motor do bot parou de dar sinal no meio de um turno. Discreto, na própria conversa: diz
// quanto tempo faz e o que a pessoa pode fazer (esperar ou parar). Parar usa o mesmo comando do botão Parar, na conversa onde o aviso está (num grupo, o grupo).
import { HStack } from '../../../components/Stack'
import { Button } from '../../../components/Controls'
import { Icon } from '../../../components/Icon'
import { Text } from '../../../components/Text'
import type { Entry } from '@shared/models'
import { superficieIdioma, useIdioma, useT } from '../../../lib/fork-i18n/estado'
import { useStoreRef as useStore } from '../../../store/context'
import { lerMotorMudo, textoMotorMudo } from './motor-mudo'

export function AvisoMotorMudo({ entry }: { entry: Entry }) {
  const idioma = useIdioma()
  const t = useT()
  const store = useStore()
  const aviso = lerMotorMudo(entry)
  if (!aviso) return null
  return (
    <div className="chat-notice idioma-superficie" {...superficieIdioma(idioma)}>
      <HStack gap={8} className="chat-pill glass-raised" role="status">
        <Icon name="clock" size={12} color="var(--context-ink-2, var(--ink-2))" />
        <Text size="footnote" tone="secondary" className="selectable">{textoMotorMudo(t, aviso)}</Text>
        {aviso.estado === 'mudo' ? <Button kind="secondary" onClick={() => store.stop(entry.botId)}>{t('conversa.mensagens.parar')}</Button> : null}
      </HStack>
    </div>
  )
}
