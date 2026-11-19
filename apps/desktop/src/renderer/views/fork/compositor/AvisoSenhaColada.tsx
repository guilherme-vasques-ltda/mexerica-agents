// MX-214 (tela 4 da proposta Cofre seguro, MX-146): o aviso acima da caixa de mensagem quando o que a pessoa colou
// parece senha, chave ou token. Só desenha e devolve a decisão; não vê nem guarda o trecho.
import { Button } from '../../../components/Controls'
import { Icon } from '../../../components/Icon'
import { HStack, VStack } from '../../../components/Stack'
import { Text } from '../../../components/Text'
import { useT } from '../../../lib/fork-i18n/estado'

/** `nomeBot`: o bot desta conversa; `null` num canal ou grupo (o texto fala dos bots em geral). */
export function AvisoSenhaColada({ nomeBot, recusada, envioBarrado, mover, naoESenha }: { nomeBot: string | null; recusada: boolean; envioBarrado: boolean; mover: () => void; naoESenha: () => void }) {
  const t = useT()
  return (
    <div className="aviso-senha-faixa">
    <div className="aviso-senha" role="alert" aria-label={t('conversa.compositor.senha.regiao')}>
      <span className="aviso-senha-selo" aria-hidden="true"><Icon name="shield" size={18} /></span>
      <VStack gap={4} grow>
        <Text size="body" weight="semibold">{t('conversa.compositor.senha.titulo')}</Text>
        <Text size="callout" tone="secondary" dir="auto">
          {nomeBot ? t('conversa.compositor.senha.texto', { bot: nomeBot }) : t('conversa.compositor.senha.textoGeral')}
        </Text>
        <HStack gap={8} wrap className="aviso-senha-acoes">
          <Button onClick={mover}><Icon name="lock" size={12} />{t('conversa.compositor.senha.mover')}</Button>
          <Button kind="secondary" onClick={naoESenha}>{t('conversa.compositor.senha.naoESenha')}</Button>
        </HStack>
        <Text size="caption" tone="secondary">{t('conversa.compositor.senha.rodape')}</Text>
        {envioBarrado ? <Text size="caption" weight="semibold" role="status">{t('conversa.compositor.senha.envioBloqueado')}</Text> : null}
        {recusada ? <Text size="caption" weight="semibold">{t('conversa.compositor.senha.recusada')}</Text> : null}
      </VStack>
    </div>
    </div>
  )
}
