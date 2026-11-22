import Foundation

/// A 400 from the Hub, as the channel got it. The Hub's text can carry internal detail, so it is kept
/// only for `RoutineRefusal` to recognise and is never shown: `HostError.refused` describes itself with a
/// fixed phrase and this type prints nothing.
public struct HubRefusal: Sendable, Equatable, CustomStringConvertible, CustomDebugStringConvertible {
    let raw: String?
    init(_ raw: String?) { self.raw = raw }
    public var description: String { "HubRefusal" }
    public var debugDescription: String { "HubRefusal" }
}

/// What the person reads when the Hub refuses to save or check a routine. Only the refusals the Hub is
/// known to write for routines (`check.rs`, `schedule.rs`) are recognised, by their exact wording, and each
/// becomes a local pt-BR/en phrase. Anything else (absent, empty, unknown, or with internal detail) gets one
/// generic local phrase: no Hub text reaches the screen.
enum RoutineRefusal {
    static func message(_ refusal: HubRefusal) -> String {
        guard let text = refusal.raw?.trimmingCharacters(in: .whitespacesAndNewlines), !text.isEmpty, text.count <= 400 else {
            return generic
        }
        switch text {
        case "A rotina precisa de pelo menos um gatilho.":
            return String(localized: "A rotina precisa de pelo menos um gatilho.", bundle: .module)
        case "timeoutSeconds must be an integer between 1 and 86400":
            return String(localized: "O tempo limite deve ser um número inteiro de 1 a 86400 segundos.", bundle: .module)
        case "routine name or instruction is empty or too long":
            return String(localized: "O nome e a instrução da rotina não podem ficar vazios nem ser longos demais.", bundle: .module)
        default:
            break
        }
        if let count = capture(tooMany, in: text).flatMap({ Int($0) }) {
            return String(localized: "A rotina comporta até 20 gatilhos; esta tem \(count). Tire alguns ou divida em outra rotina.", bundle: .module)
        }
        if let label = capture(duplicate, in: text) {
            // The Hub's label is a time ("15:00") or a cron expression; only a time is repeated back.
            if label.wholeMatch(of: /\d{2}:\d{2}/) != nil {
                return String(localized: "Dois horários iguais: \(label). Troque um deles ou tire um.", bundle: .module)
            }
            return String(localized: "Dois horários iguais. Troque um deles ou tire um.", bundle: .module)
        }
        return generic
    }

    static var generic: String {
        String(localized: "O computador não aceitou esta rotina. Confira o agendamento e os campos e tente de novo.", bundle: .module)
    }

    private static let tooMany = #"^A rotina comporta até 20 gatilhos; esta tem (\d{1,4})\. Tire alguns ou divida em outra rotina\.$"#
    private static let duplicate = #"^Dois horários iguais: ([^\n]{1,80})\. Troque um deles ou tire um\.$"#

    private static func capture(_ pattern: String, in text: String) -> String? {
        guard let regex = try? NSRegularExpression(pattern: pattern),
              let match = regex.firstMatch(in: text, range: NSRange(text.startIndex..., in: text)),
              match.numberOfRanges == 2, let range = Range(match.range(at: 1), in: text) else { return nil }
        return String(text[range])
    }
}

extension HostClient {
    /// A routine call whose 400 refusals become local phrases; every other error passes through unchanged.
    func routineCall<T>(_ body: () async throws -> T) async throws -> T {
        do { return try await body() }
        catch HostError.refused(let refusal) { throw HostError.http(400, RoutineRefusal.message(refusal)) }
    }
}
