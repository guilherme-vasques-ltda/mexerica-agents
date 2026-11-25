//! Contrato estrito do cofre para a rota de chave de API: só registro cifrado,
//! ligado ao vínculo (provedor, bot e concessão), sem plaintext e sem migração.
//!
//! A leitura usa [`crate::market::vault::open_ciphertext`], que exige prefixo
//! cifrado e a mesma AAD usada na gravação; registro em texto claro é recusado
//! sem ser convertido. O ponto de ligação da frente do cofre é
//! [`aad_do_vinculo`]: quando o item de Keychain exclusivo do Hub entrar, a
//! mesma tupla versionada vira a AAD do item, e esta leitura passa a receber a
//! chave-mestra daquela identidade — nada aqui cai para outro caminho.
#[cfg(test)]
use crate::LockExt;
use crate::store::Store;
use zeroize::Zeroizing;

/// Segmento identificador aceito em vínculo e escopo: sem separador, sem
/// caminho e sem conteúdo arbitrário.
fn segmento(valor: &str) -> bool {
    !valor.is_empty()
        && valor.len() <= 128
        && valor.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}

/// Vínculo autorizado de uma chamada: quem pede e sob qual concessão.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Vinculo<'a> {
    pub bot: &'a str,
    pub concessao: &'a str,
}

impl Vinculo<'_> {
    pub(crate) fn validar(&self) -> Result<(), RecusaCofre> {
        (segmento(self.bot) && segmento(self.concessao)).then_some(()).ok_or(RecusaCofre::VinculoInvalido)
    }
}

/// AAD versionada da rota: provedor, bot e concessão, sem o modelo. O prefixo
/// `v2` separa este vínculo do AAD antigo, que só cobria o slot.
pub(crate) fn aad_do_vinculo(vinculo: &Vinculo<'_>) -> Option<String> {
    vinculo.validar().ok().map(|()| format!("rota:v2:anthropic:{}:{}", vinculo.bot, vinculo.concessao))
}

/// Motivo fechado de recusa do cofre. Nenhum variante carrega conteúdo.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RecusaCofre {
    /// Vínculo ausente ou com caractere fora do contrato.
    VinculoInvalido,
    /// Não há registro para este vínculo.
    Ausente,
    /// Registro existe, mas não é cifrado ou a chave não abre com esta AAD.
    Recusado,
    /// Cofre trancado ou indisponível no sistema operacional.
    Indisponivel,
    /// Registro abriu, mas não é uma chave de API válida.
    FormatoInvalido,
}

impl RecusaCofre {
    pub(crate) fn codigo(self) -> &'static str {
        match self {
            Self::VinculoInvalido => "cofre_vinculo",
            Self::Ausente => "cofre_ausente",
            Self::Recusado => "cofre_recusado",
            Self::Indisponivel => "cofre_indisponivel",
            Self::FormatoInvalido => "cofre_formato",
        }
    }
}

impl std::fmt::Display for RecusaCofre {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::VinculoInvalido => "O vínculo da chamada não confere com o contrato do cofre.",
            Self::Ausente => "Não há chave de API cadastrada para este vínculo.",
            Self::Recusado => "O registro do cofre não abriu para este vínculo.",
            Self::Indisponivel => "O cofre está trancado ou indisponível neste sistema.",
            Self::FormatoInvalido => "O registro do cofre não contém uma chave de API válida.",
        })
    }
}

impl std::error::Error for RecusaCofre {}

/// Segredo em memória, zerado ao sair de escopo. Sem `Debug`, `Clone` ou
/// serialização: quem usa recebe a referência só para montar o cabeçalho.
pub(crate) struct Segredo(Zeroizing<String>);

impl Segredo {
    /// Aceita apenas o formato de chave de API já reconhecido pela automação;
    /// login pessoal e texto livre são recusados.
    pub(crate) fn novo(texto: String) -> Result<Self, RecusaCofre> {
        let texto = Zeroizing::new(texto);
        crate::fork::background_anthropic::api_key(texto.as_str()).map_err(|_| RecusaCofre::FormatoInvalido)?;
        Ok(Self(texto))
    }

    /// Uso restrito ao transporte: o cabeçalho recebe a chave marcada como
    /// sensível e o corpo da requisição nunca a vê.
    pub(crate) fn expor(&self) -> &str {
        self.0.as_str()
    }
}

impl std::fmt::Debug for Segredo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("<credencial>")
    }
}

/// Leitura da credencial para uma chamada. Sem métodos de listagem, escrita ou
/// exportação: o adaptador só consegue abrir o vínculo que já tem.
pub(crate) trait Cofre {
    fn abrir(&self, vinculo: &Vinculo<'_>) -> Result<Segredo, RecusaCofre>;
}

/// Leitura pelo cofre cifrado do próprio Hub, com a AAD do vínculo.
pub(crate) struct CofreDoHub<'a> {
    store: &'a Store,
}

impl<'a> CofreDoHub<'a> {
    pub(crate) fn novo(store: &'a Store) -> Self {
        Self { store }
    }
}

impl Cofre for CofreDoHub<'_> {
    fn abrir(&self, vinculo: &Vinculo<'_>) -> Result<Segredo, RecusaCofre> {
        let slot = aad_do_vinculo(vinculo).ok_or(RecusaCofre::VinculoInvalido)?;
        let bruto = self.store.kv_read(&slot).map_err(|_| RecusaCofre::Indisponivel)?.ok_or(RecusaCofre::Ausente)?;
        let texto = crate::market::vault::open_ciphertext(self.store, &slot, &bruto).map_err(|erro| {
            if crate::market::vault::indisponivel(&erro) { RecusaCofre::Indisponivel } else { RecusaCofre::Recusado }
        })?;
        Segredo::novo(texto)
    }
}

/// Gravação usada pelo cadastro local; a leitura acima é a única que o
/// adaptador tem. Mantida aqui para que o teste do contrato use exatamente a
/// mesma AAD da abertura.
#[cfg(test)]
pub(crate) fn gravar(store: &Store, vinculo: &Vinculo<'_>, chave: &str) -> anyhow::Result<()> {
    let slot = aad_do_vinculo(vinculo).ok_or_else(|| anyhow::anyhow!("vínculo inválido"))?;
    crate::market::vault::write(store, &slot, chave)
}

/// A leitura nunca acontece sem chave-mestra em memória: `open_ciphertext`
/// devolve erro de cofre indisponível, e nenhum caminho daqui gera chave.
#[cfg(test)]
pub(crate) fn trancar(store: &Store) {
    *store.secret_key.locked() = None;
}
