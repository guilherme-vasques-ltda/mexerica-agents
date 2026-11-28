// DON-907 C1: Ajustes → Conversa. Cada parte é uma seção (Limites de uso, Arquivar sozinho, Contexto) e entra como
// mais um `CardSection` aqui. docs/features/chat-messages.md, "Atalho de envio".
import { superficieIdioma, useIdioma, useT, type Tradutor } from '../../../lib/fork-i18n/estado'
import { CardForm, CardSection, ChoicePicker, ValueRow } from '../../../components/Controls'
import { usePref } from '../../../lib/prefs'
import { SwitchRow } from '../../bots/parts'
import { isMac } from '../../settings/parts'
import type { SendMode } from '../../thread/composer-keyboard'
import { LimitesDeUsoSection } from './LimitesDeUso'
import { ArquivarSozinhoSection } from './arquivar/ArquivarSozinho'
import { conversaPrefs } from './preferencias'
import { ContextoAjuste } from '../contexto/ContextoAjuste'

const mod = isMac ? '⌘' : 'Ctrl'

export function ConversaSettings() {
  const t = useT()
  const idioma = useIdioma()
  const open = window.codync.appScheme === 'mexerica-open'
  const [sendMode, setSendMode] = usePref(conversaPrefs.sendMode)
  const [slashSkills, setSlashSkills] = usePref(conversaPrefs.slashSkills)
  const options: { id: SendMode; label: string }[] = [
    { id: 'enter', label: t('conversa.envio.modoEnter') },
    { id: 'mod-multiline', label: t('conversa.envio.modoVariasLinhas', { mod }) },
    { id: 'mod', label: t('conversa.envio.modoSempre', { mod }) },
  ]
  return (
    <div {...superficieIdioma(idioma)} className="idioma-superficie" style={{ display: 'contents' }}><CardForm>
      <CardSection title={t('conversa.envio.titulo')} footer={sendFooter(sendMode, t)}>
        <ValueRow label={t('conversa.envio.atalho')} detail={t('conversa.envio.atalhoDetalhe')}>
          <ChoicePicker language={superficieIdioma(idioma)} selection={sendMode} options={options} onChange={setSendMode} />
        </ValueRow>
      </CardSection>
      {/* DON-907 C4: lida e gravada no Hub (docs/reference/design/don-907-c4/README.md). */}
      <LimitesDeUsoSection />
      {/* DON-907 C5: o prazo é do Hub (docs/reference/design/don-907-c5/README.md). */}
      {!open && <ArquivarSozinhoSection />}
      {!open && <ContextoAjuste />}
      {!open && <CardSection title={t('conversa.avancado.titulo')}>
        <SwitchRow title={t('conversa.avancado.skillsMenu')} detail={t('conversa.avancado.skillsMenuDetalhe')} on={slashSkills} onChange={setSlashSkills} />
      </CardSection>}
    </CardForm></div>
  )
}

/** O que cada modo faz, em uma linha, abaixo da escolha. */
function sendFooter(mode: SendMode, t: Tradutor) {
  switch (mode) {
    case 'enter':
      return t('conversa.envio.rodapeEnter')
    case 'mod-multiline':
      return t('conversa.envio.rodapeVariasLinhas', { mod })
    case 'mod':
      return t('conversa.envio.rodapeSempre', { mod })
  }
}
