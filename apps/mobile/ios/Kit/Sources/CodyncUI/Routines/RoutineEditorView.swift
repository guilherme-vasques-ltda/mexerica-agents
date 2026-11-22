// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import CodyncKit
import SwiftUI

struct RoutineEditorView: View {
    let botId: String
    let routine: Routine?
    /// Called after a save (the routine) or a delete (nil).
    let saved: (Routine?) -> Void
    @Environment(BotStore.self) private var model
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @Environment(\.locale) private var locale
    @State private var name = ""
    @State private var instruction = ""
    @State private var schedule = RoutineScheduleDraft()
    @State private var timeout = "3600"
    @State private var busy = false
    @State private var error: String?
    @State private var advanced = false
    @State private var loaded = false
    @State private var preview: RoutineSchedulePreview?
    @State private var previewError: String?
    @State private var checkedSchedule: RoutineScheduleDraft?
    /// The schedule as the editor opened it and the triggers it came from: while they match, Save sends the triggers back untouched.
    @State private var loadedSchedule: RoutineScheduleDraft?
    @State private var loadedTriggers: [RoutineTrigger] = []
    @State private var confirmDelete = false
    @State private var started = false
    @State private var pickingZone = false
    /// Any text field is being edited; cleared on save so nothing sits under the keyboard.
    @FocusState private var typing: Bool

    /// Schedule or webhook, as in Claude Code routines. Anything else the bot set up
    /// (a one-off time, an interval, events, several triggers) stays as it is until replaced.
    private var options: [(String, String)] {
        var values = [("cron", String(localized: "Agendamento", bundle: .module)), ("webhook", String(localized: "Webhook", bundle: .module))]
        if let routine, keepsOriginal {
            values.insert(("keep", routine.scheduleText(locale: locale)), at: 0)
        }
        return values
    }

    private var keepsOriginal: Bool {
        guard let routine, routine.triggers.count == 1 else { return routine != nil && !(routine?.triggers.isEmpty ?? true) }
        return !["cron", "webhook"].contains(routine.triggers[0].type)
    }

    private var canSave: Bool {
        !name.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty &&
        !instruction.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty && !model.isOffline && !busy &&
        loaded && checkedSchedule == schedule && previewError == nil
    }

    var body: some View {
        VStack(spacing: 0) {
            ModalHeader(routine == nil ? String(localized: "Configurar rotina", bundle: .module) : String(localized: "Editar rotina", bundle: .module))
            Group {
                ScrollView { form }.scrollDismissesKeyboard(.interactively)
            }
            .disabled(busy || !loaded)
            footer
        }
        .font(.system(size: 16))
        .foregroundStyle(Palette.text)
        .background(Palette.background)
        .codyncDialog(String(localized: "Excluir rotina?", bundle: .module), isPresented: $confirmDelete, message: String(localized: "Isso exclui a rotina e interrompe suas próximas execuções. Não é possível desfazer.", bundle: .module)) {
            [DialogAction(String(localized: "Excluir rotina", bundle: .module), destructive: true) {
                guard let routine else { return }
                Task {
                    do {
                        try await model.client?.routineAction("deleteRoutine", botId: botId, id: routine.id)
                        saved(nil)
                    } catch { self.error = error.localizedDescription }
                }
            }]
        }
        .codyncSheet(isPresented: $pickingZone) {
            TimeZonePicker(selection: schedule.zone) {
                schedule.zone = $0
                pickingZone = false
            }
        }
        .task {
            if let routine {
                name = routine.name
                instruction = routine.instruction
                timeout = String(routine.timeoutSeconds ?? 3600)
            }
            await loadSchedule()
        }
        .task(id: schedule) {
            guard loaded else { return }
            do { try await Task.sleep(for: .milliseconds(300)) }
            catch { return }
            await checkSchedule()
        }
    }

    private var form: some View {
            VStack(alignment: .leading, spacing: 24) {
                VStack(alignment: .leading, spacing: 14) {
                    Field(String(localized: "Nome", bundle: .module)) {
                        TextField(String(localized: "Nome", bundle: .module), text: $name, prompt: Text(String(localized: "Ex.: resumo da manhã", bundle: .module)).foregroundStyle(Palette.secondary)).routineInput().focused($typing)
                    }
                    Field(String(localized: "Instrução", bundle: .module)) {
                        TextField(String(localized: "Instrução", bundle: .module), text: $instruction, prompt: Text(String(localized: "Descreva o que este bot deve fazer em cada execução.", bundle: .module)).foregroundStyle(Palette.secondary), axis: .vertical)
                            .lineLimit(3...8).routineInput().focused($typing)
                    }
                }
                VStack(alignment: .leading, spacing: 14) {
                    Field(String(localized: "Quando executar", bundle: .module)) {
                        ChoicePicker(selection: $schedule.kind, options: options.map { (id: $0.0, label: $0.1) }, fitsAvailableWidth: true)
                    }
                    if loaded { triggerFields }
                    else { Label(String(localized: "Carregando agendamento…", bundle: .module), systemImage: "clock").foregroundStyle(Palette.secondary) }
                }
                VStack(alignment: .leading, spacing: 12) {
                    Button {
                        withAnimation(Motion.reduced(Motion.layout, reduceMotion)) { advanced.toggle() }
                    } label: {
                        HStack(spacing: 6) {
                            Text(String(localized: "Configurações de execução", bundle: .module))
                            Image(systemName: "chevron.down").font(.caption2.weight(.semibold))
                                .rotationEffect(.degrees(advanced ? 180 : 0))
                            Spacer()
                            if !advanced { Text("Tempo limite \(Routine.timeoutText(timeout, locale: locale))", bundle: .module).foregroundStyle(Palette.tertiary) }
                        }
                        .font(.subheadline)
                        .foregroundStyle(Palette.secondary)
                        .padding(.leading, 4)
                        .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                    .accessibilityValue(advanced ? String(localized: "Expandido", bundle: .module) : String(localized: "Recolhido", bundle: .module))
                    if advanced {
                        Field(String(localized: "Tempo limite (segundos)", bundle: .module)) { TextField("3600", text: $timeout).routineInput().focused($typing) }
                        Text(String(localized: "A execução é interrompida após esse tempo. De 1 segundo a 24 horas.", bundle: .module))
                            .font(.caption).foregroundStyle(Palette.secondary).padding(.leading, 4)
                    }
                }
            }
            .padding(20)
            .frame(maxWidth: .infinity, alignment: .leading)
    }

    private func checkSchedule() async {
        let requested = schedule
        preview = nil
        previewError = nil
        checkedSchedule = nil
        do {
            guard let client = model.client else { throw URLError(.notConnectedToInternet) }
            let result = try await client.routineSchedule(draft: requested)
            try Task.checkCancellation()
            guard schedule == requested else { return }
            preview = result
            checkedSchedule = requested
        } catch {
            guard !Task.isCancelled, schedule == requested else { return }
            previewError = error.localizedDescription
        }
    }

    private func loadSchedule() async {
        guard let client = model.client else { error = String(localized: "Conecte-se a este computador para carregar o agendamento.", bundle: .module); return }
        do {
            let triggers = routine?.triggers ?? []
            let result = try await client.routineSchedule(triggers: triggers, timeZone: TimeZone.current.identifier)
            schedule = result.draft.forMobileEditor()
            loadedSchedule = schedule
            loadedTriggers = triggers
            preview = result
            checkedSchedule = result.draft
            loaded = true
            error = nil
        } catch { self.error = error.localizedDescription }
    }

    @ViewBuilder private var triggerFields: some View {
        switch schedule.kind {
        case "cron":
            cronFields
        case "webhook":
            if let routine, routine.triggers.contains(where: { $0.type == "webhook" || $0.type == "event" }) {
                RoutineWebhookPanel(botId: botId, routineId: routine.id)
            } else {
                Text(String(localized: "Executa sempre que algo é enviado ao seu endereço. Salve para obter o endereço e a chave.", bundle: .module))
                    .font(.caption).foregroundStyle(Palette.secondary).padding(.leading, 4)
                    .fixedSize(horizontal: false, vertical: true)
            }
        default:
            Text(String(localized: "Configurado pelo bot. Escolha Agendamento ou Webhook para substituí-lo.", bundle: .module))
                .font(.caption).foregroundStyle(Palette.secondary).padding(.leading, 4)
        }
    }

    /// A frequency with its time, or cron written directly; the host compiles, checks and describes it.
    private var cronFields: some View {
        VStack(alignment: .leading, spacing: 14) {
            ViewThatFits(in: .horizontal) {
                HStack(alignment: .top, spacing: 10) { frequencyFields }
                VStack(alignment: .leading, spacing: 14) { frequencyFields }
            }
            VStack(alignment: .leading, spacing: 8) {
                if schedule.calendarStyle == "custom" {
                    ViewThatFits(in: .horizontal) {
                        HStack(alignment: .top, spacing: 10) { expressionField; zoneField }
                        VStack(alignment: .leading, spacing: 14) { expressionField; zoneField }
                    }
                } else {
                    zoneField
                }
                schedulePreview.padding(.leading, 4)
            }
        }
    }

    @ViewBuilder private var frequencyFields: some View {
        Field(String(localized: "Repetir", bundle: .module)) {
            ChoicePicker(selection: Binding(get: { schedule.calendarStyle }, set: { selectFrequency($0) }), options: RoutineScheduleDraft.repeatChoices)
        }
        switch schedule.calendarStyle {
        case "hourly":
            Field(String(localized: "No minuto", bundle: .module)) {
                ChoicePicker(selection: $schedule.minute, options: (0..<60).map { (id: $0, label: String(format: ":%02d", $0)) })
            }
        case "daily", "weekdays":
            timeField
        case "weekly":
            Field(String(localized: "Em", bundle: .module)) {
                // Monday first; the host counts 0 = Sunday … 6 = Saturday, like cron.
                ChoicePicker(selection: $schedule.weekday,
                             options: [1, 2, 3, 4, 5, 6, 0].map { (id: $0, label: weekdaySymbols[$0]) })
            }
            timeField
        default:
            EmptyView()
        }
    }

    private var weekdaySymbols: [String] {
        var calendar = Calendar.current
        calendar.locale = locale
        return calendar.standaloneWeekdaySymbols
    }

    private var timeField: some View {
        Field(String(localized: "Às", bundle: .module)) {
            DatePicker(String(localized: "Às", bundle: .module), selection: $schedule.timeOfDay, displayedComponents: .hourAndMinute)
                .labelsHidden()
                .environment(\.timeZone, .gmt)
        }
    }

    /// Custom starts from the cron the host compiled for the checked choice it replaces.
    private func selectFrequency(_ style: String) {
        let compiled = checkedSchedule == schedule ? preview?.cronExpression : nil
        withAnimation(Motion.reduced(Motion.layout, reduceMotion)) { schedule = schedule.repeating(style, compiled: compiled) }
    }

    private var expressionField: some View {
        Field(String(localized: "Cron", bundle: .module)) {
            TextField(String(localized: "Cron", bundle: .module), text: $schedule.expression, prompt: Text("0 9 * * 1-5").foregroundStyle(Palette.secondary))
                .font(.body.monospaced()).autocorrectionDisabled().routineInput().focused($typing)
                .frame(minWidth: 220)
        }
    }

    private var zoneField: some View {
        Field(String(localized: "Fuso horário", bundle: .module)) {
            Button { pickingZone = true } label: {
                HStack(spacing: 6) {
                    Text(schedule.zone).lineLimit(1)
                    Text(TimeZones.offset(schedule.zone)).foregroundStyle(Palette.secondary)
                    Image(systemName: "chevron.down").font(.caption2.weight(.semibold)).foregroundStyle(Palette.secondary)
                }
                .foregroundStyle(Palette.text)
                .outlinedPill()
            }
            .buttonStyle(.plain)
        }
    }

    /// One quiet line under the fields: what the host read, or why it can't.
    @ViewBuilder private var schedulePreview: some View {
        let preview = checkedSchedule == schedule ? preview : nil
        if let previewError {
            Text(previewError).font(.caption).foregroundStyle(Palette.danger)
                .fixedSize(horizontal: false, vertical: true)
        } else if let preview {
            VStack(alignment: .leading, spacing: 2) {
                Text(preview.summaryText(locale: locale)).font(.caption.weight(.medium))
                if let next = preview.nextRunAt {
                    Text("Próxima execução \(Date(milliseconds: next).formatted(.dateTime.day().month(.abbreviated).year().hour().minute().locale(locale)))", bundle: .module)
                        .font(.caption).foregroundStyle(Palette.secondary)
                }
                if let warning = preview.warning {
                    Text(warning).font(.caption).foregroundStyle(Palette.secondary)
                }
            }
        } else if schedule.calendarStyle == "custom" {
            Text(String(localized: "minuto  hora  dia  mês  dia da semana", bundle: .module)).font(.caption.monospaced()).foregroundStyle(Palette.tertiary)
        }
    }

    private var footer: some View {
        VStack(alignment: .leading, spacing: 12) {
            if previewError != nil {
                Button(String(localized: "Verificar agendamento novamente", bundle: .module)) { Task { await checkSchedule() } }
                    .buttonStyle(.plain).disabled(model.isOffline)
            }
            if !loaded, error != nil {
                Button(String(localized: "Tentar carregar o agendamento novamente", bundle: .module)) { Task { await loadSchedule() } }.buttonStyle(.plain)
            }
            if schedule.kind != "cron", let previewError {
                Text(previewError).font(.caption).foregroundStyle(Palette.danger)
            }
            if let error {
                Label(error, systemImage: "exclamationmark.circle")
                    .font(.callout).foregroundStyle(Palette.danger).textSelection(.enabled)
            }
            if model.isOffline {
                Label(String(localized: "Conecte-se a este computador para salvar sua rotina.", bundle: .module), systemImage: "wifi.slash")
                    .font(.caption).foregroundStyle(Palette.danger)
            }
            if loaded, !model.isOffline,
               name.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty ||
               instruction.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                Text(String(localized: "Digite um nome e uma instrução acima para salvar.", bundle: .module))
                    .font(.caption).foregroundStyle(Palette.secondary)
            }
            HStack(spacing: 16) { routineActions; saveButton }
        }
        .padding(20)
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(.bottom, 16)
        .background(Palette.background)
    }

    /// An existing routine's own actions: delete and run it now.
    @ViewBuilder private var routineActions: some View {
        if let routine {
            IconButton(String(localized: "Excluir rotina", bundle: .module), systemImage: "trash") { confirmDelete = true }
            IconButton(started ? String(localized: "Iniciada", bundle: .module) : String(localized: "Testar execução", bundle: .module), systemImage: started ? "checkmark" : "play.circle") {
                Task {
                    do {
                        try await model.client?.routineAction("runRoutine", botId: botId, id: routine.id)
                        withAnimation(Motion.reduced(Motion.hover, reduceMotion)) { started = true }
                    } catch { self.error = error.localizedDescription }
                }
            }
            .disabled(started || model.isOffline)
        }
    }

    private var saveButton: some View {
        Button(action: save) {
            HStack(spacing: 8) {
                if busy { Spinner() }
                Text(busy ? String(localized: "Salvando…", bundle: .module) : routine == nil ? String(localized: "Criar rotina", bundle: .module) : String(localized: "Salvar alterações", bundle: .module))
                    .fixedSize(horizontal: false, vertical: true)
            }
            .frame(maxWidth: .infinity, minHeight: 26)
        }
        .buttonStyle(.primary)
        .disabled(!canSave)
        .keyboardShortcut("s", modifiers: .command)
    }

    private func save() {
        guard let client = model.client, canSave else { return }
        // Saving ends editing (a new webhook routine stays open on its URL and key, not under the keyboard).
        typing = false
        busy = true
        error = nil
        Task {
            defer { busy = false }
            do {
                let result: Routine
                switch RoutineSavePlan.make(current: schedule, loaded: loadedSchedule, loadedTriggers: loadedTriggers) {
                case let .triggers(original):
                    result = try await client.saveRoutine(botId: botId, id: routine?.id, name: name,
                        instruction: instruction, triggers: original, timeoutSeconds: timeout)
                case let .draft(draft):
                    result = try await client.saveRoutine(botId: botId, id: routine?.id, name: name,
                        instruction: instruction, schedule: draft, timeoutSeconds: timeout)
                }
                saved(result)
            } catch { self.error = error.localizedDescription }
        }
    }
}

private extension View {
    func routineInput() -> some View {
        textFieldStyle(.plain)
            .padding(12)
            .background(Palette.bubbleUser, in: RoundedRectangle(cornerRadius: 12))
    }
}
