// O ícone de um conector, um só em todo lugar (lista de Conectores, Credenciais e Ferramentas do bot): a marca do serviço
// do catálogo, em arquivo local, quando a conta veio de uma ficha; senão o globo neutro. Nada sai do computador para
// descobrir ícone (sem favicon).
import type { InstalledConnector } from '../../marketplace/market-models'
import { ServiceLogo } from '../../marketplace/pieces'

export function LogoConector({ conector, size = 46 }: { conector: Pick<InstalledConnector, 'name' | 'marca'>; size?: number }) {
  return <ServiceLogo name={conector.name} marca={conector.marca} size={size} />
}
