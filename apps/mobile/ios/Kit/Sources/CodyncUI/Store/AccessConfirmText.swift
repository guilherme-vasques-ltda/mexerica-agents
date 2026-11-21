import CodyncKit
import Foundation

/// The words of the confirmation-code step in the product's six languages, picked by locale.
/// The rest of the access screen is fixed Portuguese; this small table can be checked per language.
public enum AccessConfirmText {
    public enum Key: CaseIterable, Sendable {
        /// The Mac showed a code: type it here.
        case prompt
        /// Accessibility name of the code field.
        case field
        case confirm
        /// Wrong code; `%d` is how many tries are left.
        case wrong
        /// Five wrong codes: the request was withdrawn.
        case tooMany
        /// For a Mac that doesn't show a code yet.
        case oldMac
        case startOver
        /// The code can't be checked on this iPhone: nothing was saved.
        case unusable
        /// A new request got 409: an earlier approval is still active on the Mac. The page is named as the
        /// desktop names it (`ajustes.tela` › `ajustes.pagina.computadores`); change them together.
        case alreadyApproved
        /// Home banner and Settings row while the Mac's code is awaited: where to type it.
        case typeOnPhone
    }

    /// The person's first preferred language, not the app's (the app itself is in Portuguese).
    public static var preferredLocale: Locale { Locale(identifier: Locale.preferredLanguages.first ?? "pt-BR") }

    public static func text(_ key: Key, attemptsLeft: Int = 0, locale: Locale = AccessConfirmText.preferredLocale) -> String {
        let table = tables[language(locale)] ?? tables["pt"]!
        let value = table[key] ?? tables["pt"]![key]!
        return value.replacingOccurrences(of: "%d", with: String(attemptsLeft))
    }

    /// Why asking for access failed, in the person's language when it's this step's own case (a 409 from
    /// the request route); otherwise the error's fixed phrase. Never text from the cloud.
    public static func requestErrorText(_ error: any Error, locale: Locale = AccessConfirmText.preferredLocale) -> String {
        if let error = error as? CloudError, error.code == "deviceAlreadyApproved" { return text(.alreadyApproved, locale: locale) }
        return error.localizedDescription
    }

    /// A pending request's line (home banner, Settings row): the code to type on the Mac while the Mac
    /// hasn't approved; once it did and shows its own code, where to type that one.
    public static func waitingDetail(_ ticket: AccessTicket, awaitingCode: Bool, locale: Locale = AccessConfirmText.preferredLocale) -> String {
        if awaitingCode { return text(.typeOnPhone, locale: locale) }
        return "Digite no Mac o código \(ticket.code.prefix(3)) \(ticket.code.suffix(3))"
    }

    /// pt · en · es · fr · zh · ar; any other language falls back to Portuguese, like the rest of the screen.
    static func language(_ locale: Locale) -> String {
        let code = locale.language.languageCode?.identifier ?? "pt"
        return tables[code] == nil ? "pt" : code
    }

    static let tables: [String: [Key: String]] = [
        "pt": [
            .prompt: "O Mac mostrou um código. Digite-o aqui",
            .field: "Código de confirmação",
            .confirm: "Confirmar",
            .wrong: "Código errado. Tentativas restantes: %d",
            .tooMany: "Código errado vezes demais. O pedido foi cancelado. Peça acesso de novo.",
            .oldMac: "Se o Mac não mostrou um código, atualize o Mexerica no Mac.",
            .startOver: "Cancelar e começar de novo",
            .unusable: "Não foi possível conferir o código. Nada foi salvo. Peça acesso de novo.",
            .typeOnPhone: "Digite no iPhone o código que apareceu no Mac",
            .alreadyApproved: "Este iPhone já foi aprovado nesse Mac, mas não terminou a ligação. No Mac, em Ajustes › Computadores e dispositivos, remova este iPhone e peça de novo.",
        ],
        "en": [
            .prompt: "Your Mac is showing a code. Type it here",
            .field: "Confirmation code",
            .confirm: "Confirm",
            .wrong: "Wrong code. Tries left: %d",
            .tooMany: "Too many wrong codes. The request was cancelled. Ask for access again.",
            .oldMac: "If your Mac didn't show a code, update Mexerica on the Mac.",
            .startOver: "Cancel and start over",
            .unusable: "The code couldn't be checked. Nothing was saved. Ask for access again.",
            .typeOnPhone: "Type on the iPhone the code that appeared on the Mac",
            .alreadyApproved: "This iPhone was already approved on that Mac, but the connection wasn't finished. On the Mac, in Settings › Computers and devices, remove this iPhone and ask again.",
        ],
        "es": [
            .prompt: "Tu Mac muestra un código. Escríbelo aquí",
            .field: "Código de confirmación",
            .confirm: "Confirmar",
            .wrong: "Código incorrecto. Intentos restantes: %d",
            .tooMany: "Demasiados códigos incorrectos. La solicitud se canceló. Vuelve a pedir acceso.",
            .oldMac: "Si tu Mac no mostró un código, actualiza Mexerica en el Mac.",
            .startOver: "Cancelar y empezar de nuevo",
            .unusable: "No se pudo comprobar el código. No se guardó nada. Vuelve a pedir acceso.",
            .typeOnPhone: "Escribe en el iPhone el código que apareció en el Mac",
            .alreadyApproved: "Este iPhone ya se aprobó en ese Mac, pero la conexión no terminó. En el Mac, en Ajustes › Ordenadores y dispositivos, quita este iPhone y vuelve a pedir acceso.",
        ],
        "fr": [
            .prompt: "Votre Mac affiche un code. Saisissez-le ici",
            .field: "Code de confirmation",
            .confirm: "Confirmer",
            .wrong: "Code incorrect. Essais restants : %d",
            .tooMany: "Trop de codes incorrects. La demande a été annulée. Demandez l’accès à nouveau.",
            .oldMac: "Si votre Mac n’a pas affiché de code, mettez à jour Mexerica sur le Mac.",
            .startOver: "Annuler et recommencer",
            .unusable: "Impossible de vérifier le code. Rien n’a été enregistré. Demandez l’accès à nouveau.",
            .typeOnPhone: "Saisissez sur l’iPhone le code apparu sur le Mac",
            .alreadyApproved: "Cet iPhone a déjà été approuvé sur ce Mac, mais la connexion n’a pas abouti. Sur le Mac, dans Réglages › Ordinateurs et appareils, retirez cet iPhone et redemandez l’accès.",
        ],
        "zh": [
            .prompt: "Mac 上显示了一个代码，请在这里输入",
            .field: "确认码",
            .confirm: "确认",
            .wrong: "代码错误。剩余次数：%d",
            .tooMany: "错误次数过多，请求已取消。请重新申请访问。",
            .oldMac: "如果 Mac 上没有显示代码，请在 Mac 上更新 Mexerica。",
            .startOver: "取消并重新开始",
            .unusable: "无法核对代码，未保存任何内容。请重新申请访问。",
            .typeOnPhone: "在 iPhone 上输入 Mac 上出现的代码",
            .alreadyApproved: "这台 iPhone 已在那台 Mac 上获得批准，但连接没有完成。请在 Mac 的“设置 › 电脑和设备”中移除这台 iPhone，然后重新申请。",
        ],
        "ar": [
            .prompt: "يعرض جهاز Mac رمزًا. اكتبه هنا",
            .field: "رمز التأكيد",
            .confirm: "تأكيد",
            .wrong: "رمز خاطئ. المحاولات المتبقية: %d",
            .tooMany: "أُدخل رمز خاطئ مرات كثيرة. أُلغي الطلب. اطلب الوصول من جديد.",
            .oldMac: "إذا لم يعرض جهاز Mac رمزًا، فحدِّث Mexerica على جهاز Mac.",
            .startOver: "إلغاء والبدء من جديد",
            .unusable: "تعذّر التحقق من الرمز. لم يُحفظ أي شيء. اطلب الوصول من جديد.",
            .typeOnPhone: "اكتب على الـ iPhone الرمز الذي ظهر على جهاز Mac",
            .alreadyApproved: "تمت الموافقة على هذا الـ iPhone في جهاز Mac هذا، لكن الربط لم يكتمل. على جهاز Mac، من «الإعدادات › الحواسيب والأجهزة»، أزِل هذا الـ iPhone ثم اطلب من جديد.",
        ],
    ]
}

extension AccountStore {
    /// The home banner's line for a pending request (RootView).
    public func accessBannerDetail(_ ticket: AccessTicket, locale: Locale = AccessConfirmText.preferredLocale) -> String {
        AccessConfirmText.waitingDetail(ticket, awaitingCode: isAwaitingConfirmCode(ticket), locale: locale)
    }

    /// The Settings row's line for a computer with a request in progress (SettingsView); nil without one.
    public func accessRowDetail(_ id: ComputerID, locale: Locale = AccessConfirmText.preferredLocale) -> String? {
        guard let ticket = pendingAccess[id] else { return nil }
        return AccessConfirmText.waitingDetail(ticket, awaitingCode: isAwaitingConfirmCode(ticket), locale: locale)
    }

    /// The Mac approved this very request and shows its code (`accessConfirm`).
    private func isAwaitingConfirmCode(_ ticket: AccessTicket) -> Bool {
        accessConfirm[ticket.computerId]?.requestId == ticket.requestId
    }
}
