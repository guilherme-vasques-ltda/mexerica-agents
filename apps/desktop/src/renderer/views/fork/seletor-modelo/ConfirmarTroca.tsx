// DON-907 C9 (D375): a confirmação da troca numa conversa longa, exportada para o compositor e o criar bot usarem a
// mesma (chaves `conversa.seletor.confirmar.*`). Quem chama decide se pergunta (`precisaConfirmar`, limiar de
// 50 mil tokens a reler) e o que fazer em Trocar.
import { Dialog } from '../../../components/Overlay'
import { superficieIdioma, useIdioma, useT } from '../../../lib/fork-i18n/estado'

export function ConfirmarTroca({ aberto, onTrocar, onCancelar }: { aberto: boolean; onTrocar: () => void; onCancelar: () => void }) {
  const t = useT()
  const idioma = useIdioma()
  return (
    <Dialog language={superficieIdioma(idioma)} open={aberto} title={t('conversa.seletor.confirmar.titulo')} message={t('conversa.seletor.confirmar.texto')}
      cancel={t('conversa.seletor.confirmar.agoraNao')} onClose={onCancelar}
      actions={[{ title: t('conversa.seletor.confirmar.trocar'), action: onTrocar }]} />
  )
}
