// MX-277: o cartão acima da caixa depois de "Mover para o cofre": janela segura aberta, "Guardado no cofre" ou falha.
// Só estado, nunca valor: a senha é digitada na janela segura e não passa pelo chat.
import { IconButton } from '../../../components/Controls'
import { Icon } from '../../../components/Icon'
import { VStack } from '../../../components/Stack'
import { Text } from '../../../components/Text'
import { useT } from '../../../lib/fork-i18n/estado'
import type { FaseCofre } from './cofre-estado'

export function AvisoCofre({ fase, fechar }: { fase: FaseCofre; fechar: () => void }) {
  const t = useT()
  const guardado = fase === 'guardado'
  const aberta = fase === 'abrindo' || fase === 'aberta'
  const titulo = guardado ? t('extensoes.cofre.guardado') : aberta ? t('conversa.compositor.cofre.abertaTitulo') : t('conversa.compositor.cofre.erroTitulo')
  const texto = guardado ? t('conversa.compositor.cofre.guardadoTexto')
    : aberta ? t('conversa.compositor.cofre.abertaTexto')
    : t(fase === 'semSuporte' ? 'extensoes.cofre.erro.semSuporte' : 'extensoes.cofre.erro.abrir')
  return (
    <div className="aviso-senha-faixa">
      <div className={`aviso-senha${guardado ? ' is-guardado' : ''}`} role="status" aria-label={titulo}>
        <span className="aviso-senha-selo" aria-hidden="true"><Icon name={guardado ? 'checkmark.shield' : 'lock'} size={18} /></span>
        <VStack gap={4} grow>
          <Text size="body" weight="semibold">{titulo}</Text>
          <Text size="callout" tone="secondary" dir="auto">{texto}</Text>
        </VStack>
        {aberta ? null : <IconButton title={t('conversa.compositor.cofre.fechar')} icon="xmark" size={24} onClick={fechar} />}
      </div>
    </div>
  )
}
