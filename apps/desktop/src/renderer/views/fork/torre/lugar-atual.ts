// VG-02: um lugar atual por vez. Abrir o Mission Board fecha Rotinas, Times, Ajustes e Extensões; abrir um deles fecha o Mission Board.
import { routinesScreen } from '../../rotinas-central/navigation'
import { settingsScreen } from '../ajustes/lugar-atual'
import { extensoesScreen } from '../extensoes/lugar-atual'
import { timesScreen } from '../times/navigation'
import { torreScreen } from './navigation'

export { torreScreen }

const outras = [routinesScreen, timesScreen, settingsScreen, extensoesScreen]
torreScreen.subscribe(() => { if (torreScreen.open) for (const o of outras) o.close(false) })
for (const o of outras) o.subscribe(() => { if (o.open) torreScreen.close(false) })
